use anyhow::{anyhow, Result};
use crc32fast::Hasher as Crc32Hasher;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use tokio::sync::RwLock;
use tokio::fs::{self, File};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time::sleep;
use uuid::Uuid;

const MAGIC: &[u8; 4] = b"SWFT";
const HEADER_LEN: usize = 24;
const SMALL_FILE_LIMIT: u64 = 1_048_576;
const INLINE_FILE_LIMIT: u64 = 65_536; // files <= 64KB are inlined in the meta packet
const RECONNECT_MAX_RETRIES: usize = 5;
const RECONNECT_BASE_DELAY_MS: u64 = 300;

#[derive(Debug, Clone, Serialize)]
pub struct TransferStatus {
    pub total_files: usize,
    pub sent_files: usize,
    pub current_path: Option<String>,
    pub bytes_sent: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PullProgress {
    pub entry_id: String,
    pub name: String,
    pub received_bytes: u64,
    pub total_bytes: u64,
    pub entry_received_bytes: u64,
    pub entry_total_bytes: u64,
}

/// Cancellation token for pull operations.
#[derive(Clone)]
pub struct CancelToken {
    cancelled: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self { cancelled: Arc::new(AtomicBool::new(false)) }
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, AtomicOrdering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(AtomicOrdering::Relaxed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirFileInfo {
    pub path: String,
    pub size: u64,
}

#[derive(Clone)]
pub struct TransportHandle {
    pub port: u16,
    pub machine_id: String,
    #[allow(dead_code)]
    inbound_pool: ConnectionPool,
    outbound_pool: ConnectionPool,
    shared: SharedIndex,
}

#[derive(Clone, Default)]
struct ConnectionPool {
    inner: Arc<Mutex<HashMap<String, TcpStream>>>,
}

#[derive(Clone, Default)]
struct SharedIndex {
    inner: Arc<RwLock<HashMap<String, SharedEntry>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub size: u64,
    pub modified: u64,
    /// Relative path from the shared root (e.g. "MyFolder/sub/file.txt").
    /// For top-level files this equals `name`. For directories this is the
    /// root folder name (e.g. "MyFolder") with is_dir = true.
    #[serde(default)]
    pub relative_path: String,
    /// True when this entry represents a directory root.
    #[serde(default)]
    pub is_dir: bool,
}

#[derive(Debug, Clone)]
struct FileEntry {
    path: PathBuf,
    size: u64,
}

#[derive(Debug, Clone, Copy)]
enum PacketType {
    FileMeta = 1,
    FileChunk = 2,
    SmallFileStream = 3,
    ListRequest = 4,
    ListResponse = 5,
    PullRequest = 6,
    PullStream = 7,
    Error = 8,
    Goodbye = 9,
    /// A single file within a directory pull. Payload format is the same as
    /// PullStream but the name field contains the relative path within the
    /// directory (e.g. "MyFolder/sub/file.txt").
    DirFileStream = 10,
    /// Signals the end of a directory pull.
    DirEnd = 11,
    /// Request the flat file list for a directory entry (by id).
    DirListRequest = 12,
    /// Response: JSON array of relative path strings.
    DirListResponse = 13,
    /// Single file pull, small enough to inline content in the meta packet.
    /// Payload: meta (same as PullStream) + file_bytes + 4-byte CRC32 (big-endian).
    PullInline = 14,
    /// Directory file, small enough to inline. Same layout as PullInline.
    DirFileInline = 15,
    /// Heartbeat packet for firewall traversal - helps devices behind firewalls be discovered
    Heartbeat = 16,
}

pub async fn start_transfer(
    handle: &TransportHandle,
    paths: Vec<String>,
    target_ip: String,
    target_port: u16,
    max_mbps: u64,
) -> Result<()> {
    let mut queue = VecDeque::new();
    for input in paths {
        collect_files(Path::new(&input), &mut queue)?;
    }

    let total_files = queue.len();
    if total_files == 0 {
        return Err(anyhow!("No files to transfer"));
    }

    let mut status = TransferStatus {
        total_files,
        sent_files: 0,
        current_path: None,
        bytes_sent: 0,
    };

    let addr = format!("{}:{}", target_ip, target_port);
    let mut stream = handle
        .outbound_pool
        .get_or_connect(&addr, &addr)
        .await?;

    while let Some(entry) = queue.pop_front() {
        status.current_path = Some(entry.path.display().to_string());
        let offset = match request_resume_offset(&mut stream, &entry).await {
            Ok(value) => value,
            Err(_) => {
                stream = handle
                    .outbound_pool
                    .get_or_connect(&addr, &addr)
                    .await?;
                request_resume_offset(&mut stream, &entry).await.unwrap_or(0)
            }
        };

        let send_result = if entry.size <= SMALL_FILE_LIMIT {
            send_small_file_stream(&mut stream, &entry, offset, &mut status, max_mbps).await
        } else {
            send_file_chunked(&mut stream, &entry, offset, &mut status, max_mbps).await
        };

        if send_result.is_err() {
            stream = handle
                .outbound_pool
                .get_or_connect(&addr, &addr)
                .await?;
            let offset = request_resume_offset(&mut stream, &entry).await.unwrap_or(0);
            if entry.size <= SMALL_FILE_LIMIT {
                send_small_file_stream(&mut stream, &entry, offset, &mut status, max_mbps).await?;
            } else {
                send_file_chunked(&mut stream, &entry, offset, &mut status, max_mbps).await?;
            }
        }

        status.sent_files += 1;
    }

    handle.outbound_pool.insert(addr, stream).await;
    Ok(())
}

pub async fn send_goodbye(target_ip: String, target_port: u16) {
  let addr = format!("{}:{}", target_ip, target_port);
  // Use tokio::time::timeout to avoid hanging on unreachable devices
  let result = tokio::time::timeout(
    Duration::from_millis(500),
    TcpStream::connect(&addr)
  ).await;
  
  if let Ok(Ok(mut stream)) = result {
    let _ = write_packet(&mut stream, PacketType::Goodbye, 0, &[]).await;
  }
}

pub async fn start_listener(port: Option<u16>) -> Result<TransportHandle> {
    let bind_port = port.unwrap_or(0);  // 0 表示随机端口
    let listener = TcpListener::bind(("0.0.0.0", bind_port)).await?;
    let port = listener.local_addr()?.port();
    let pool = ConnectionPool::default();
    let outbound_pool = ConnectionPool::default();
    let shared = SharedIndex::default();
    let machine_id = generate_machine_id().await;

    let pool_clone = pool.clone();
    let shared_clone = shared.clone();
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    let key = addr.ip().to_string();
                    pool_clone.insert(key.clone(), stream).await;
                    let _ = tokio::spawn(handle_connection(key, pool_clone.clone(), shared_clone.clone()));
                }
                Err(_) => break,
            }
        }
    });

    Ok(TransportHandle {
        port,
        machine_id,
        inbound_pool: pool,
        outbound_pool,
        shared,
    })
}

async fn handle_connection(peer_key: String, pool: ConnectionPool, shared: SharedIndex) -> Result<()> {
    let mut stream = match pool.take(&peer_key).await {
        Some(stream) => stream,
        None => return Ok(()),
    };

    let mut current_path: Option<PathBuf> = None;
    let mut current_size: u64 = 0;

    loop {
        let mut header = [0u8; HEADER_LEN];
        if stream.read_exact(&mut header).await.is_err() {
            break;
        }
        if &header[..4] != MAGIC {
            return Err(anyhow!("Invalid magic"));
        }
        let packet_type = u16::from_be_bytes([header[4], header[5]]);
        let offset = u64::from_be_bytes([
            header[10], header[11], header[12], header[13], header[14], header[15], header[16], header[17],
        ]);
        let len = u64::from_be_bytes([0, 0, header[18], header[19], header[20], header[21], header[22], header[23]]);

        if packet_type == PacketType::FileMeta as u16 {
            let mut payload = vec![0u8; len as usize];
            stream.read_exact(&mut payload).await?;
            let (path, size, suggested_offset) = parse_meta(&payload)?;
            let target_path = PathBuf::from(path);
            let existing = file_existing_len(&target_path).await;
            let resume_offset = existing.max(suggested_offset).min(size);
            stream.write_all(&resume_offset.to_be_bytes()).await?;
            current_path = Some(target_path);
            current_size = size;
        } else if packet_type == PacketType::SmallFileStream as u16 {
            let mut payload = vec![0u8; len as usize];
            stream.read_exact(&mut payload).await?;
            let (path, size, offset) = parse_meta(&payload)?;
            receive_streamed_file(&mut stream, PathBuf::from(path), size, offset, 0, 0, |_| {}).await?;
        } else if packet_type == PacketType::FileChunk as u16 {
            let mut payload = vec![0u8; len as usize];
            stream.read_exact(&mut payload).await?;
            if let Some(path) = current_path.clone() {
                let capped_offset = offset.min(current_size);
                receive_chunk_payload(path, capped_offset, &payload).await?;
            }
        } else if packet_type == PacketType::ListRequest as u16 {
            let mut payload = vec![0u8; len as usize];
            stream.read_exact(&mut payload).await?;
            let list = shared.list().await;
            let json = serde_json::to_vec(&list)?;
            write_packet(&mut stream, PacketType::ListResponse, 0, &json).await?;
        } else if packet_type == PacketType::PullRequest as u16 {
            let mut payload = vec![0u8; len as usize];
            stream.read_exact(&mut payload).await?;
            let id = String::from_utf8_lossy(&payload).to_string();
            let entry = match shared.get(&id).await {
                Some(entry) => entry,
                None => {
                    let msg = format!("Entry not found: {}", id);
                    write_packet(&mut stream, PacketType::Error, 0, msg.as_bytes()).await?;
                    continue;
                }
            };

            if entry.is_dir {
                // Send all files in the directory tree, each as a DirFileStream packet.
                let dir_path = PathBuf::from(&entry.path);
                let mut file_queue = VecDeque::new();
                collect_files(&dir_path, &mut file_queue)?;
                // parent of dir_path, so relative paths start with the dir name
                let base = dir_path.parent().unwrap_or(&dir_path);
                let mut revoked = false;
                for file_entry in file_queue {
                    // Re-check if shared entry still exists (may have been cleared)
                    if shared.get(&id).await.is_none() {
                        let msg = format!("Shared entry revoked: {}", id);
                        write_packet(&mut stream, PacketType::Error, 0, msg.as_bytes()).await?;
                        revoked = true;
                        break;
                    }
                    let rel = file_entry.path.strip_prefix(base)
                        .unwrap_or(&file_entry.path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    let file_meta = match fs::metadata(&file_entry.path).await {
                        Ok(m) => m,
                        Err(_) => continue,
                    };
                    let size = file_meta.len();
                    let modified = file_meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let mut meta_payload = Vec::new();
                    meta_payload.extend_from_slice(&(rel.len() as u16).to_be_bytes());
                    meta_payload.extend_from_slice(rel.as_bytes());
                    meta_payload.extend_from_slice(&size.to_be_bytes());
                    meta_payload.extend_from_slice(&0u64.to_be_bytes()); // offset
                    meta_payload.extend_from_slice(&modified.to_be_bytes());
                    if size <= INLINE_FILE_LIMIT {
                        // Read entire file, compute CRC32, inline everything
                        let file_bytes = match fs::read(&file_entry.path).await {
                            Ok(b) => b,
                            Err(_) => continue,
                        };
                        let crc = crc32_of(&file_bytes);
                        meta_payload.extend_from_slice(&file_bytes);
                        meta_payload.extend_from_slice(&crc.to_be_bytes());
                        write_packet(&mut stream, PacketType::DirFileInline, 0, &meta_payload).await?;
                    } else {
                        // Stream large file, append CRC32 after data
                        let crc = match crc32_of_file(&file_entry.path).await {
                            Ok(c) => c,
                            Err(_) => continue,
                        };
                        meta_payload.extend_from_slice(&crc.to_be_bytes());
                        write_packet(&mut stream, PacketType::DirFileStream, 0, &meta_payload).await?;
                        let mut file = match File::open(&file_entry.path).await {
                            Ok(f) => f,
                            Err(_) => continue,
                        };
                        let mut buf = vec![0u8; 64 * 1024];
                        loop {
                            let read = file.read(&mut buf).await?;
                            if read == 0 { break; }
                            stream.write_all(&buf[..read]).await?;
                        }
                    }
                }
                if !revoked {
                    write_packet(&mut stream, PacketType::DirEnd, 0, &[]).await?;
                }
            } else {
                let path = PathBuf::from(entry.path.clone());
                let metadata = match fs::metadata(&path).await {
                    Ok(metadata) => metadata,
                    Err(_) => {
                        let msg = format!("Source file missing: {}", entry.name);
                        write_packet(&mut stream, PacketType::Error, 0, msg.as_bytes()).await?;
                        continue;
                    }
                };
                let size = metadata.len();
                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(entry.modified);

                if size != entry.size || modified != entry.modified {
                    let msg = format!("Source file changed: {}", entry.name);
                    write_packet(&mut stream, PacketType::Error, 0, msg.as_bytes()).await?;
                    continue;
                }

                let mut meta_payload = Vec::new();
                meta_payload.extend_from_slice(&(entry.name.len() as u16).to_be_bytes());
                meta_payload.extend_from_slice(entry.name.as_bytes());
                meta_payload.extend_from_slice(&size.to_be_bytes());
                meta_payload.extend_from_slice(&0u64.to_be_bytes()); // offset
                meta_payload.extend_from_slice(&modified.to_be_bytes());

                if size <= INLINE_FILE_LIMIT {
                    let file_bytes = match fs::read(&path).await {
                        Ok(b) => b,
                        Err(_) => {
                            let msg = format!("Failed to read: {}", entry.name);
                            write_packet(&mut stream, PacketType::Error, 0, msg.as_bytes()).await?;
                            continue;
                        }
                    };
                    let crc = crc32_of(&file_bytes);
                    meta_payload.extend_from_slice(&file_bytes);
                    meta_payload.extend_from_slice(&crc.to_be_bytes());
                    write_packet(&mut stream, PacketType::PullInline, 0, &meta_payload).await?;
                } else {
                    let crc = match crc32_of_file(&path).await {
                        Ok(c) => c,
                        Err(_) => {
                            let msg = format!("Failed to checksum: {}", entry.name);
                            write_packet(&mut stream, PacketType::Error, 0, msg.as_bytes()).await?;
                            continue;
                        }
                    };
                    meta_payload.extend_from_slice(&crc.to_be_bytes());
                    write_packet(&mut stream, PacketType::PullStream, 0, &meta_payload).await?;

                    let mut file = match File::open(&path).await {
                        Ok(file) => file,
                        Err(_) => {
                            let msg = format!("Failed to open source file: {}", entry.name);
                            write_packet(&mut stream, PacketType::Error, 0, msg.as_bytes()).await?;
                            continue;
                        }
                    };
                    let mut buf = vec![0u8; 64 * 1024];
                    loop {
                        let read = file.read(&mut buf).await?;
                        if read == 0 { break; }
                        stream.write_all(&buf[..read]).await?;
                    }
                }
            }
        } else if packet_type == PacketType::DirListRequest as u16 {
            let mut payload = vec![0u8; len as usize];
            stream.read_exact(&mut payload).await?;
            let id = String::from_utf8_lossy(&payload).to_string();
            let entry = shared.get(&id).await;
            let file_list: Vec<DirFileInfo> = if let Some(entry) = entry {
                if entry.is_dir {
                    let dir_path = PathBuf::from(&entry.path);
                    let base = dir_path.parent().unwrap_or(&dir_path).to_path_buf();
                    let mut queue = VecDeque::new();
                    let _ = collect_files(&dir_path, &mut queue);
                    queue.iter().map(|f| {
                        let rel = f.path.strip_prefix(&base)
                            .unwrap_or(&f.path)
                            .to_string_lossy()
                            .replace('\\', "/");
                        DirFileInfo { path: rel, size: f.size }
                    }).collect()
                } else { vec![] }
            } else { vec![] };
            let json = serde_json::to_vec(&file_list).unwrap_or_default();
            write_packet(&mut stream, PacketType::DirListResponse, 0, &json).await?;
        } else if packet_type == PacketType::Heartbeat as u16 {
            // Heartbeat packet - just acknowledge and continue
            // This helps devices behind firewalls be discovered by allowing
            // the firewall to establish a bidirectional connection
            let _ = write_packet(&mut stream, PacketType::Heartbeat, 0, &[]).await;
        } else if packet_type == PacketType::Goodbye as u16 {
            break;
        } else {
            if len > 0 {
                let mut sink = vec![0u8; len as usize];
                stream.read_exact(&mut sink).await?;
            }
        }
    }

    Ok(())
}

pub async fn add_shared(handle: &TransportHandle, paths: Vec<String>) -> Result<Vec<SharedEntry>> {
    let mut new_entries = Vec::new();
    for input in paths {
        let path = PathBuf::from(&input);
        let meta = fs::metadata(&path).await?;
        if meta.is_dir() {
            // Create a single directory entry representing the whole folder tree.
            // All files inside are encoded with their relative path so the
            // receiver can reconstruct the full directory structure.
            let dir_name = path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string());
            let modified = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            // Compute total size of all files inside.
            let total_size = dir_total_size(&path)?;
            let id = format!("dir:{}:{}", path.display(), modified);
            let shared = SharedEntry {
                id,
                name: dir_name.clone(),
                path: path.display().to_string(),
                size: total_size,
                modified,
                relative_path: dir_name,
                is_dir: true,
            };
            handle.shared.insert(shared.clone()).await;
            new_entries.push(shared);
        } else {
            let size = meta.len();
            let shared = build_shared_entry(&path, size).await?;
            handle.shared.insert(shared.clone()).await;
            new_entries.push(shared);
        }
    }
    Ok(new_entries)
}

fn dir_total_size(path: &Path) -> Result<u64> {
    let mut total = 0u64;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let meta = std::fs::metadata(entry.path())?;
        if meta.is_dir() {
            total += dir_total_size(&entry.path())?;
        } else {
            total += meta.len();
        }
    }
    Ok(total)
}

pub async fn list_shared(handle: &TransportHandle) -> Result<Vec<SharedEntry>> {
    Ok(handle.shared.list().await)
}

/// For a directory entry, return all files with relative paths and sizes.
pub async fn list_dir_files(handle: &TransportHandle, entry_id: String) -> Result<Vec<DirFileInfo>> {
    let entry = handle.shared.get(&entry_id).await
        .ok_or_else(|| anyhow!("Entry not found: {}", entry_id))?;
    if !entry.is_dir {
        return Ok(vec![]);
    }
    let dir_path = PathBuf::from(&entry.path);
    let base = dir_path.parent().unwrap_or(&dir_path).to_path_buf();
    let mut queue = VecDeque::new();
    collect_files(&dir_path, &mut queue)?;
    let mut result = Vec::new();
    for file in queue {
        let rel = file.path.strip_prefix(&base)
            .unwrap_or(&file.path)
            .to_string_lossy()
            .replace('\\', "/");
        result.push(DirFileInfo { path: rel, size: file.size });
    }
    Ok(result)
}

pub async fn clear_shared(handle: &TransportHandle) -> Result<()> {
    handle.shared.clear().await;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct ConflictInfo {
    pub has_conflict: bool,
    pub conflicting_files: Vec<String>,
    pub total_conflict_size: u64,
}

/// Check whether pulling an entry into `dest_dir` would overwrite existing files.
/// For directories, calls `fetch_remote_dir_files` to get the file list and checks each.
/// For single files, checks `dest_dir/name`.
pub async fn check_pull_conflict(
    handle: &TransportHandle,
    entry_name: String,
    entry_is_dir: bool,
    entry_id: String,
    target_ip: String,
    target_port: u16,
    dest_dir: String,
) -> Result<ConflictInfo> {
    let dest = PathBuf::from(&dest_dir);
    let mut conflicting_files = Vec::new();
    let mut total_conflict_size: u64 = 0;

    if entry_is_dir {
        let files = fetch_remote_dir_files(handle, entry_id, target_ip, target_port).await?;
        for file_info in &files {
            let local_path = dest.join(file_info.path.replace('/', std::path::MAIN_SEPARATOR_STR));
            if local_path.exists() {
                conflicting_files.push(file_info.path.clone());
                total_conflict_size += file_info.size;
            }
        }
    } else {
        let local_path = dest.join(&entry_name);
        if local_path.exists() {
            if let Ok(meta) = std::fs::metadata(&local_path) {
                total_conflict_size += meta.len();
            }
            conflicting_files.push(entry_name);
        }
    }

    Ok(ConflictInfo {
        has_conflict: !conflicting_files.is_empty(),
        conflicting_files,
        total_conflict_size,
    })
}

pub async fn fetch_remote_dir_files(
    handle: &TransportHandle,
    entry_id: String,
    target_ip: String,
    target_port: u16,
) -> Result<Vec<DirFileInfo>> {
    let addr = format!("{}:{}", target_ip, target_port);
    let mut stream = handle
        .outbound_pool
        .get_or_connect(&addr, &addr)
        .await?;
    write_packet(&mut stream, PacketType::DirListRequest, 0, entry_id.as_bytes()).await?;
    let (packet_type, payload) = read_packet(&mut stream).await?;
    if packet_type != PacketType::DirListResponse as u16 {
        return Err(anyhow!("Invalid dir list response"));
    }
    let list: Vec<DirFileInfo> = serde_json::from_slice(&payload)?;
    handle.outbound_pool.insert(addr, stream).await;
    Ok(list)
}

pub async fn fetch_remote_list(
    handle: &TransportHandle,
    target_ip: String,
    target_port: u16,
) -> Result<Vec<SharedEntry>> {
    let addr = format!("{}:{}", target_ip, target_port);
    let mut stream = handle
        .outbound_pool
        .get_or_connect(&addr, &addr)
        .await?;
    write_packet(&mut stream, PacketType::ListRequest, 0, &[]).await?;

    let (packet_type, payload) = read_packet(&mut stream).await?;
    if packet_type != PacketType::ListResponse as u16 {
        return Err(anyhow!("Invalid list response"));
    }
    let list: Vec<SharedEntry> = serde_json::from_slice(&payload)?;
    handle.outbound_pool.insert(addr, stream).await;
    Ok(list)
}

pub async fn pull_file(
    handle: &TransportHandle,
    entry_id: String,
    target_ip: String,
    target_port: u16,
    dest_dir: String,
    max_mbps: u64,
    entry_total_size: u64,
    cancel: CancelToken,
    mut on_progress: impl FnMut(PullProgress) + Send,
) -> Result<String> {
    let addr = format!("{}:{}", target_ip, target_port);
    let mut attempt = 0;

    loop {
        if cancel.is_cancelled() {
            return Err(anyhow!("Pull cancelled"));
        }
        let mut stream = handle
            .outbound_pool
            .get_or_connect(&addr, &addr)
            .await?;
        write_packet(&mut stream, PacketType::PullRequest, 0, entry_id.as_bytes()).await?;

        let (packet_type, payload) = read_packet(&mut stream).await?;
        if packet_type == PacketType::Error as u16 {
            let msg = String::from_utf8_lossy(&payload).to_string();
            return Err(anyhow!(msg));
        }

        // --- Directory pull (DirFileInline or DirFileStream packets until DirEnd) ---
        if packet_type == PacketType::DirFileInline as u16
            || packet_type == PacketType::DirFileStream as u16
        {
            let entry_total_bytes = entry_total_size;
            let mut entry_received_bytes: u64 = 0;
            let mut current_pt = packet_type;
            let mut current_payload = payload;
            let mut received_name = String::new();
            loop {
                if current_pt == PacketType::DirEnd as u16 {
                    break;
                }
                if current_pt == PacketType::Error as u16 {
                    let msg = String::from_utf8_lossy(&current_payload).to_string();
                    return Err(anyhow!(msg));
                }
                if cancel.is_cancelled() {
                    return Err(anyhow!("Pull cancelled"));
                }
                let is_inline = current_pt == PacketType::DirFileInline as u16;
                let is_stream = current_pt == PacketType::DirFileStream as u16;
                if !is_inline && !is_stream {
                    return Err(anyhow!("Unexpected packet in dir pull: {}", current_pt));
                }
                let (rel_path, size, offset, _modified, expected_crc) = parse_pull_meta(&current_payload)?;
                let meta_end = 2 + rel_path.len() + 24;
                let rel_path_local = rel_path.replace('/', std::path::MAIN_SEPARATOR_STR);
                let mut target_path = PathBuf::from(&dest_dir);
                target_path.push(&rel_path_local);
                if let Some(parent) = target_path.parent() {
                    fs::create_dir_all(parent).await.ok();
                }
                if received_name.is_empty() {
                    received_name = rel_path.split('/').next().unwrap_or(&rel_path).to_string();
                }
                if is_inline {
                    let content_end = current_payload.len().saturating_sub(4);
                    let file_bytes = &current_payload[meta_end..content_end];
                    let expected_crc = u32::from_be_bytes([
                        current_payload[content_end],
                        current_payload[content_end + 1],
                        current_payload[content_end + 2],
                        current_payload[content_end + 3],
                    ]);
                    let actual_crc = crc32_of(file_bytes);
                    if actual_crc != expected_crc {
                        return Err(anyhow!("CRC32 mismatch for {}", rel_path));
                    }
                    fs::write(&target_path, file_bytes).await?;
                    on_progress(PullProgress {
                        entry_id: entry_id.clone(),
                        name: rel_path.clone(),
                        received_bytes: size,
                        total_bytes: size,
                        entry_received_bytes: entry_received_bytes + size,
                        entry_total_bytes,
                    });
                    entry_received_bytes += size;
                } else {
                    let cancel_ref = &cancel;
                    let entry_recv_before = entry_received_bytes;
                    receive_streamed_file(&mut stream, target_path, size, offset, expected_crc, max_mbps, |received| {
                        if cancel_ref.is_cancelled() { return; }
                        on_progress(PullProgress {
                            entry_id: entry_id.clone(),
                            name: rel_path.clone(),
                            received_bytes: received,
                            total_bytes: size,
                            entry_received_bytes: entry_recv_before + received,
                            entry_total_bytes,
                        });
                    })
                    .await?;
                    entry_received_bytes += size;
                }
                let (next_pt, next_payload) = read_packet(&mut stream).await?;
                current_pt = next_pt;
                current_payload = next_payload;
            }
            handle.outbound_pool.insert(addr, stream).await;
            return Ok(received_name);
        }

        // --- Single file pull ---
        let is_inline = packet_type == PacketType::PullInline as u16;
        let is_stream = packet_type == PacketType::PullStream as u16;
        if !is_inline && !is_stream {
            return Err(anyhow!("Invalid pull response: {}", packet_type));
        }

        let (name, size, offset, _modified, expected_crc) = parse_pull_meta(&payload)?;
        let meta_end = 2 + name.len() + 24;
        let mut target_path = PathBuf::from(&dest_dir);
        target_path.push(name.clone());

        if is_inline {
            let content_end = payload.len().saturating_sub(4);
            let file_bytes = &payload[meta_end..content_end];
            let expected_crc = u32::from_be_bytes([
                payload[content_end],
                payload[content_end + 1],
                payload[content_end + 2],
                payload[content_end + 3],
            ]);
            let actual_crc = crc32_of(file_bytes);
            if actual_crc != expected_crc {
                return Err(anyhow!("CRC32 mismatch for {}", name));
            }
            fs::write(&target_path, file_bytes).await?;
            on_progress(PullProgress {
                entry_id: entry_id.clone(),
                name: name.clone(),
                received_bytes: size,
                total_bytes: size,
                entry_received_bytes: size,
                entry_total_bytes: entry_total_size,
            });
            handle.outbound_pool.insert(addr, stream).await;
            return Ok(name);
        }

        let cancel_ref = &cancel;
        let result = receive_streamed_file(&mut stream, target_path, size, offset, expected_crc, max_mbps, |received| {
            if cancel_ref.is_cancelled() { return; }
            on_progress(PullProgress {
                entry_id: entry_id.clone(),
                name: name.clone(),
                received_bytes: received,
                total_bytes: size,
                entry_received_bytes: received,
                entry_total_bytes: entry_total_size,
            });
        })
        .await;

        match result {
            Ok(_) => {
                handle.outbound_pool.insert(addr, stream).await;
                return Ok(name);
            }
            Err(err) => {
                if attempt >= RECONNECT_MAX_RETRIES {
                    return Err(err);
                }
                attempt += 1;
                let delay = RECONNECT_BASE_DELAY_MS * attempt as u64;
                sleep(Duration::from_millis(delay)).await;
            }
        }
    }
}

async fn build_shared_entry(path: &Path, size: u64) -> Result<SharedEntry> {
    let metadata = fs::metadata(path).await?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());
    let id = format!("{}:{}", path.display(), modified);
    Ok(SharedEntry {
        id,
        relative_path: name.clone(),
        name,
        path: path.display().to_string(),
        size,
        modified,
        is_dir: false,
    })
}

fn parse_meta(payload: &[u8]) -> Result<(String, u64, u64)> {
    if payload.len() < 2 {
        return Err(anyhow!("Invalid meta payload"));
    }
    let name_len = u16::from_be_bytes([payload[0], payload[1]]) as usize;
    if payload.len() < 2 + name_len + 16 {
        return Err(anyhow!("Invalid meta payload size"));
    }
    let name = String::from_utf8_lossy(&payload[2..2 + name_len]).to_string();
    let size = u64::from_be_bytes([
        payload[2 + name_len],
        payload[3 + name_len],
        payload[4 + name_len],
        payload[5 + name_len],
        payload[6 + name_len],
        payload[7 + name_len],
        payload[8 + name_len],
        payload[9 + name_len],
    ]);
    let offset = u64::from_be_bytes([
        payload[10 + name_len],
        payload[11 + name_len],
        payload[12 + name_len],
        payload[13 + name_len],
        payload[14 + name_len],
        payload[15 + name_len],
        payload[16 + name_len],
        payload[17 + name_len],
    ]);
    Ok((name, size, offset))
}

fn parse_pull_meta(payload: &[u8]) -> Result<(String, u64, u64, u64, u32)> {
    if payload.len() < 2 {
        return Err(anyhow!("Invalid meta payload"));
    }
    let name_len = u16::from_be_bytes([payload[0], payload[1]]) as usize;
    if payload.len() < 2 + name_len + 28 {
        return Err(anyhow!("Invalid meta payload size"));
    }
    let name = String::from_utf8_lossy(&payload[2..2 + name_len]).to_string();
    let size = u64::from_be_bytes([
        payload[2 + name_len],
        payload[3 + name_len],
        payload[4 + name_len],
        payload[5 + name_len],
        payload[6 + name_len],
        payload[7 + name_len],
        payload[8 + name_len],
        payload[9 + name_len],
    ]);
    let offset = u64::from_be_bytes([
        payload[10 + name_len],
        payload[11 + name_len],
        payload[12 + name_len],
        payload[13 + name_len],
        payload[14 + name_len],
        payload[15 + name_len],
        payload[16 + name_len],
        payload[17 + name_len],
    ]);
    let modified = u64::from_be_bytes([
        payload[18 + name_len],
        payload[19 + name_len],
        payload[20 + name_len],
        payload[21 + name_len],
        payload[22 + name_len],
        payload[23 + name_len],
        payload[24 + name_len],
        payload[25 + name_len],
    ]);
    let crc32 = u32::from_be_bytes([
        payload[26 + name_len],
        payload[27 + name_len],
        payload[28 + name_len],
        payload[29 + name_len],
    ]);
    Ok((name, size, offset, modified, crc32))
}

async fn file_existing_len(path: &Path) -> u64 {
    fs::metadata(path).await.map(|m| m.len()).unwrap_or(0)
}

async fn receive_streamed_file(
    stream: &mut TcpStream,
    path: PathBuf,
    size: u64,
    offset: u64,
    expected_crc: u32,
    max_mbps: u64,
    mut on_progress: impl FnMut(u64),
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.ok();
    }

    let mut file = if offset > 0 {
        match File::open(&path).await {
            Ok(file) => file,
            Err(_) => File::create(&path).await?,
        }
    } else {
        File::create(&path).await?
    };

    if offset > 0 {
        file.seek(std::io::SeekFrom::Start(offset)).await?;
    }

    let mut remaining = size.saturating_sub(offset);
    let mut received = offset;
    let mut buf = vec![0u8; 64 * 1024];
    let throttle = Throttle::new(max_mbps);
    let mut hasher = Crc32Hasher::new();
    while remaining > 0 {
        let read_len = buf.len().min(remaining as usize);
        let read = stream.read(&mut buf[..read_len]).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
        file.write_all(&buf[..read]).await?;
        received += read as u64;
        on_progress(received);
        remaining -= read as u64;
        throttle.throttle(read as u64).await;
    }
    file.flush().await?;
    drop(file);

    // Verify CRC32 (skip if expected_crc == 0, used for push-mode transfers)
    if expected_crc != 0 {
        let actual_crc = hasher.finalize();
        if actual_crc != expected_crc {
            let _ = fs::remove_file(&path).await;
            return Err(anyhow!(
                "CRC32 mismatch for {}: expected {:08x}, got {:08x}",
                path.display(), expected_crc, actual_crc
            ));
        }
    }
    Ok(())
}

async fn receive_chunk_payload(path: PathBuf, offset: u64, payload: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.ok();
    }
    let mut file = if offset > 0 {
        match File::open(&path).await {
            Ok(file) => file,
            Err(_) => File::create(&path).await?,
        }
    } else {
        File::create(&path).await?
    };
    if offset > 0 {
        file.seek(std::io::SeekFrom::Start(offset)).await?;
    }
    file.write_all(payload).await?;
    Ok(())
}

fn collect_files(path: &Path, queue: &mut VecDeque<FileEntry>) -> Result<()> {
    let metadata = std::fs::metadata(path)?;
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            collect_files(&entry.path(), queue)?;
        }
    } else if metadata.is_file() {
        queue.push_back(FileEntry {
            path: path.to_path_buf(),
            size: metadata.len(),
        });
    }
    Ok(())
}

async fn connect_with_retry(addr: &str) -> Result<TcpStream> {
    let mut attempt = 0;
    loop {
        match TcpStream::connect(addr).await {
            Ok(stream) => return Ok(stream),
            Err(_err) if attempt < RECONNECT_MAX_RETRIES => {
                let delay = RECONNECT_BASE_DELAY_MS * (attempt as u64 + 1);
                sleep(Duration::from_millis(delay)).await;
                attempt += 1;
                continue;
            }
            Err(err) => return Err(err.into()),
        }
    }
}

impl ConnectionPool {
    async fn insert(&self, key: String, stream: TcpStream) {
        let mut map = self.inner.lock().await;
        map.insert(key, stream);
    }

    async fn take(&self, key: &str) -> Option<TcpStream> {
        let mut map = self.inner.lock().await;
        map.remove(key)
    }

    async fn get_or_connect(&self, key: &str, addr: &str) -> Result<TcpStream> {
        if let Some(stream) = self.take(key).await {
            return Ok(stream);
        }
        connect_with_retry(addr).await
    }
}

impl SharedIndex {
    async fn insert(&self, entry: SharedEntry) {
        let mut map = self.inner.write().await;
        map.insert(entry.id.clone(), entry);
    }

    async fn list(&self) -> Vec<SharedEntry> {
        let map = self.inner.read().await;
        map.values().cloned().collect()
    }

    async fn get(&self, id: &str) -> Option<SharedEntry> {
        let map = self.inner.read().await;
        map.get(id).cloned()
    }

    async fn clear(&self) {
        let mut map = self.inner.write().await;
        map.clear();
    }
}

async fn request_resume_offset(stream: &mut TcpStream, entry: &FileEntry) -> Result<u64> {
    let path_bytes = entry.path.to_string_lossy().as_bytes().to_vec();
    let mut payload = Vec::with_capacity(2 + path_bytes.len() + 16);
    payload.extend_from_slice(&(path_bytes.len() as u16).to_be_bytes());
    payload.extend_from_slice(&path_bytes);
    payload.extend_from_slice(&(entry.size as u64).to_be_bytes());
    payload.extend_from_slice(&0u64.to_be_bytes());

    write_packet(stream, PacketType::FileMeta, 0, &payload).await?;

    let mut offset_buf = [0u8; 8];
    stream.read_exact(&mut offset_buf).await?;
    Ok(u64::from_be_bytes(offset_buf))
}

async fn send_small_file_stream(
    stream: &mut TcpStream,
    entry: &FileEntry,
    offset: u64,
    status: &mut TransferStatus,
    max_mbps: u64,
) -> Result<()> {
    let mut file = File::open(&entry.path).await?;
    if offset > 0 {
        file.seek(std::io::SeekFrom::Start(offset)).await?;
    }

    let mut meta_payload = Vec::new();
    let path_bytes = entry.path.to_string_lossy().as_bytes().to_vec();
    meta_payload.extend_from_slice(&(path_bytes.len() as u16).to_be_bytes());
    meta_payload.extend_from_slice(&path_bytes);
    meta_payload.extend_from_slice(&(entry.size as u64).to_be_bytes());
    meta_payload.extend_from_slice(&offset.to_be_bytes());
    write_packet(stream, PacketType::SmallFileStream, 0, &meta_payload).await?;

    let mut buf = vec![0u8; 64 * 1024];
    let throttle = Throttle::new(max_mbps);
    loop {
        let read = file.read(&mut buf).await?;
        if read == 0 {
            break;
        }
        stream.write_all(&buf[..read]).await?;
        status.bytes_sent += read as u64;
        throttle.throttle(read as u64).await;
    }
    Ok(())
}

async fn send_file_chunked(
    stream: &mut TcpStream,
    entry: &FileEntry,
    offset: u64,
    status: &mut TransferStatus,
    max_mbps: u64,
) -> Result<()> {
    let mut file = File::open(&entry.path).await?;
    if offset > 0 {
        file.seek(std::io::SeekFrom::Start(offset)).await?;
    }

    let path_bytes = entry.path.to_string_lossy().as_bytes().to_vec();
    let mut meta_payload = Vec::new();
    meta_payload.extend_from_slice(&(path_bytes.len() as u16).to_be_bytes());
    meta_payload.extend_from_slice(&path_bytes);
    meta_payload.extend_from_slice(&(entry.size as u64).to_be_bytes());
    meta_payload.extend_from_slice(&offset.to_be_bytes());
    write_packet(stream, PacketType::FileMeta, 0, &meta_payload).await?;

    let mut buf = vec![0u8; 128 * 1024];
    let throttle = Throttle::new(max_mbps);
    let mut current_offset = offset;
    loop {
        let read = file.read(&mut buf).await?;
        if read == 0 {
            break;
        }
        write_packet(stream, PacketType::FileChunk, current_offset, &buf[..read]).await?;
        current_offset += read as u64;
        status.bytes_sent += read as u64;
        throttle.throttle(read as u64).await;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Throttle {
    bytes_per_sec: u64,
}

impl Throttle {
    fn new(max_mbps: u64) -> Self {
        let bytes_per_sec = if max_mbps == 0 {
            0
        } else {
            max_mbps.saturating_mul(1024 * 1024) / 8
        };
        Self { bytes_per_sec }
    }

    async fn throttle(&self, bytes: u64) {
        if self.bytes_per_sec == 0 {
            return;
        }
        let secs = bytes as f64 / self.bytes_per_sec as f64;
        if secs <= 0.0 {
            return;
        }
        let ms = (secs * 1000.0).ceil() as u64;
        if ms > 0 {
            sleep(Duration::from_millis(ms)).await;
        }
    }
}

fn crc32_of(data: &[u8]) -> u32 {
    let mut h = Crc32Hasher::new();
    h.update(data);
    h.finalize()
}

async fn crc32_of_file(path: &Path) -> Result<u32> {
    let mut file = File::open(path).await?;
    let mut h = Crc32Hasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 { break; }
        h.update(&buf[..n]);
    }
    Ok(h.finalize())
}

async fn write_packet(
    stream: &mut TcpStream,
    packet_type: PacketType,
    offset: u64,
    payload: &[u8],
) -> Result<()> {
    let mut header = [0u8; HEADER_LEN];
    header[..4].copy_from_slice(MAGIC);
    header[4..6].copy_from_slice(&(packet_type as u16).to_be_bytes());
    header[6..10].copy_from_slice(&0u32.to_be_bytes());
    header[10..18].copy_from_slice(&offset.to_be_bytes());
    let len = payload.len() as u64;
    let len_bytes = len.to_be_bytes();
    header[18..24].copy_from_slice(&len_bytes[2..8]);

    stream.write_all(&header).await?;
    stream.write_all(payload).await?;
    Ok(())
}

async fn read_packet(stream: &mut TcpStream) -> Result<(u16, Vec<u8>)> {
    let mut header = [0u8; HEADER_LEN];
    stream.read_exact(&mut header).await?;
    if &header[..4] != MAGIC {
        return Err(anyhow!("Invalid magic"));
    }
    let packet_type = u16::from_be_bytes([header[4], header[5]]);
    let len = u64::from_be_bytes([0, 0, header[18], header[19], header[20], header[21], header[22], header[23]]);
    let mut payload = vec![0u8; len as usize];
    if len > 0 {
        stream.read_exact(&mut payload).await?;
    }
    Ok((packet_type, payload))
}

async fn generate_machine_id() -> String {
    let id_file = get_machine_id_path();

    // Try to read existing ID
    if let Ok(content) = tokio::fs::read_to_string(&id_file).await {
        if !content.trim().is_empty() {
            return content.trim().to_string();
        }
    }

    // Generate new ID
    let new_id = Uuid::new_v4().to_string();

    // Try to persist it
    if let Some(parent) = id_file.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let _ = tokio::fs::write(&id_file, &new_id).await;

    new_id
}

fn get_machine_id_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        use std::path::PathBuf;
        PathBuf::from(
            std::env::var("APPDATA")
                .unwrap_or_else(|_| ".".to_string())
        ).join("SwiftShare").join(".machine_id")
    }
    #[cfg(target_os = "macos")]
    {
        use std::path::PathBuf;
        PathBuf::from(
            std::env::var("HOME")
                .unwrap_or_else(|_| ".".to_string())
        ).join("Library").join("Application Support").join("SwiftShare").join(".machine_id")
    }
    #[cfg(target_os = "linux")]
    {
        use std::path::PathBuf;
        PathBuf::from(
            std::env::var("HOME")
                .unwrap_or_else(|_| ".".to_string())
        ).join(".local").join("share").join("swiftshare").join(".machine_id")
    }
}
