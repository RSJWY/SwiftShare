use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::fs::{self, File};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time::sleep;

const MAGIC: &[u8; 4] = b"SWFT";
const HEADER_LEN: usize = 24;
const SMALL_FILE_LIMIT: u64 = 1_048_576;
const RECONNECT_MAX_RETRIES: usize = 5;
const RECONNECT_BASE_DELAY_MS: u64 = 300;
const DEFAULT_PORT: u16 = 7878;

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
}

#[derive(Clone)]
pub struct TransportHandle {
    pub port: u16,
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
}

pub async fn start_transfer(
    handle: &TransportHandle,
    paths: Vec<String>,
    target_ip: String,
    target_port: u16,
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
        .get_or_connect(&target_ip, &addr)
        .await?;

    while let Some(entry) = queue.pop_front() {
        status.current_path = Some(entry.path.display().to_string());
        let offset = match request_resume_offset(&mut stream, &entry).await {
            Ok(value) => value,
            Err(_) => {
                stream = handle
                    .outbound_pool
                    .get_or_connect(&target_ip, &addr)
                    .await?;
                request_resume_offset(&mut stream, &entry).await.unwrap_or(0)
            }
        };

        let send_result = if entry.size <= SMALL_FILE_LIMIT {
            send_small_file_stream(&mut stream, &entry, offset, &mut status).await
        } else {
            send_file_chunked(&mut stream, &entry, offset, &mut status).await
        };

        if send_result.is_err() {
            stream = handle
                .outbound_pool
                .get_or_connect(&target_ip, &addr)
                .await?;
            let offset = request_resume_offset(&mut stream, &entry).await.unwrap_or(0);
            if entry.size <= SMALL_FILE_LIMIT {
                send_small_file_stream(&mut stream, &entry, offset, &mut status).await?;
            } else {
                send_file_chunked(&mut stream, &entry, offset, &mut status).await?;
            }
        }

        status.sent_files += 1;
    }

    handle.outbound_pool.insert(target_ip, stream).await;
    Ok(())
}

pub async fn start_listener() -> Result<TransportHandle> {
    let listener = TcpListener::bind(("0.0.0.0", 0)).await?;
    let port = listener.local_addr()?.port();
    let pool = ConnectionPool::default();
    let outbound_pool = ConnectionPool::default();
    let shared = SharedIndex::default();

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
            receive_streamed_file(&mut stream, PathBuf::from(path), size, offset, |_| {}).await?;
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
            if let Some(entry) = shared.get(&id).await {
                let path = PathBuf::from(entry.path.clone());
                let metadata = fs::metadata(&path).await;
                let size = metadata.as_ref().map(|m| m.len()).unwrap_or(entry.size);
                let modified = metadata
                    .as_ref()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(entry.modified);

                if size != entry.size || modified != entry.modified {
                    let msg = format!("Source file changed: {}", entry.name);
                    write_packet(&mut stream, PacketType::Error, 0, msg.as_bytes()).await?;
                    continue;
                }

                let offset = 0u64;
                let mut meta_payload = Vec::new();
                meta_payload.extend_from_slice(&(entry.name.len() as u16).to_be_bytes());
                meta_payload.extend_from_slice(entry.name.as_bytes());
                meta_payload.extend_from_slice(&size.to_be_bytes());
                meta_payload.extend_from_slice(&offset.to_be_bytes());
                meta_payload.extend_from_slice(&modified.to_be_bytes());
                write_packet(&mut stream, PacketType::PullStream, 0, &meta_payload).await?;

                let mut file = File::open(&path).await?;
                if offset > 0 {
                    file.seek(std::io::SeekFrom::Start(offset)).await?;
                }
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    let read = file.read(&mut buf).await?;
                    if read == 0 {
                        break;
                    }
                    stream.write_all(&buf[..read]).await?;

                    let live = fs::metadata(&path).await;
                    let live_size = live.as_ref().map(|m| m.len()).unwrap_or(size);
                    let live_modified = live
                        .as_ref()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(modified);
                    if live_size != size || live_modified != modified {
                        let msg = format!("Source file changed during transfer: {}", entry.name);
                        write_packet(&mut stream, PacketType::Error, 0, msg.as_bytes()).await?;
                        break;
                    }
                }
            }
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
        if fs::metadata(&path).await.map(|m| m.is_dir()).unwrap_or(false) {
            let mut queue = VecDeque::new();
            collect_files(&path, &mut queue)?;
            while let Some(entry) = queue.pop_front() {
                let shared = build_shared_entry(&entry.path, entry.size).await?;
                handle.shared.insert(shared.clone()).await;
                new_entries.push(shared);
            }
        } else {
            let size = fs::metadata(&path).await?.len();
            let shared = build_shared_entry(&path, size).await?;
            handle.shared.insert(shared.clone()).await;
            new_entries.push(shared);
        }
    }
    Ok(new_entries)
}

pub async fn list_shared(handle: &TransportHandle) -> Result<Vec<SharedEntry>> {
    Ok(handle.shared.list().await)
}

pub async fn clear_shared(handle: &TransportHandle) -> Result<()> {
    handle.shared.clear().await;
    Ok(())
}

pub async fn fetch_remote_list(
    handle: &TransportHandle,
    target_ip: String,
    target_port: u16,
) -> Result<Vec<SharedEntry>> {
    let addr = format!("{}:{}", target_ip, target_port);
    let mut stream = handle
        .outbound_pool
        .get_or_connect(&target_ip, &addr)
        .await?;
    write_packet(&mut stream, PacketType::ListRequest, 0, &[]).await?;

    let (packet_type, payload) = read_packet(&mut stream).await?;
    if packet_type != PacketType::ListResponse as u16 {
        return Err(anyhow!("Invalid list response"));
    }
    let list: Vec<SharedEntry> = serde_json::from_slice(&payload)?;
    handle.outbound_pool.insert(target_ip, stream).await;
    Ok(list)
}

pub async fn pull_file(
    handle: &TransportHandle,
    entry_id: String,
    target_ip: String,
    target_port: u16,
    dest_dir: String,
    mut on_progress: impl FnMut(PullProgress) + Send,
) -> Result<()> {
    let addr = format!("{}:{}", target_ip, target_port);
    let mut attempt = 0;
    let mut last_error: Option<anyhow::Error> = None;

    loop {
        let mut stream = handle
            .outbound_pool
            .get_or_connect(&target_ip, &addr)
            .await?;
        write_packet(&mut stream, PacketType::PullRequest, 0, entry_id.as_bytes()).await?;

        let (packet_type, payload) = read_packet(&mut stream).await?;
        if packet_type == PacketType::Error as u16 {
            let msg = String::from_utf8_lossy(&payload).to_string();
            return Err(anyhow!(msg));
        }
        if packet_type != PacketType::PullStream as u16 {
            return Err(anyhow!("Invalid pull response"));
        }

        let (name, size, offset, _modified) = parse_pull_meta(&payload)?;
        let mut target_path = PathBuf::from(&dest_dir);
        target_path.push(name.clone());

        let result = receive_streamed_file(&mut stream, target_path, size, offset, |received| {
            on_progress(PullProgress {
                entry_id: entry_id.clone(),
                name: name.clone(),
                received_bytes: received,
                total_bytes: size,
            });
        })
        .await;

        match result {
            Ok(_) => {
                handle.outbound_pool.insert(target_ip, stream).await;
                return Ok(());
            }
            Err(err) => {
                last_error = Some(err);
                if attempt >= RECONNECT_MAX_RETRIES {
                    return Err(last_error.unwrap_or_else(|| anyhow!("Pull failed")));
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
        name,
        path: path.display().to_string(),
        size,
        modified,
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

fn parse_pull_meta(payload: &[u8]) -> Result<(String, u64, u64, u64)> {
    if payload.len() < 2 {
        return Err(anyhow!("Invalid meta payload"));
    }
    let name_len = u16::from_be_bytes([payload[0], payload[1]]) as usize;
    if payload.len() < 2 + name_len + 24 {
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
    Ok((name, size, offset, modified))
}

async fn file_existing_len(path: &Path) -> u64 {
    fs::metadata(path).await.map(|m| m.len()).unwrap_or(0)
}

async fn receive_streamed_file(
    stream: &mut TcpStream,
    path: PathBuf,
    size: u64,
    offset: u64,
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
    while remaining > 0 {
        let read_len = buf.len().min(remaining as usize);
        let read = stream.read(&mut buf[..read_len]).await?;
        if read == 0 {
            break;
        }
        file.write_all(&buf[..read]).await?;
        received += read as u64;
        on_progress(received);
        remaining -= read as u64;
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
    loop {
        let read = file.read(&mut buf).await?;
        if read == 0 {
            break;
        }
        stream.write_all(&buf[..read]).await?;
        status.bytes_sent += read as u64;
    }
    Ok(())
}

async fn send_file_chunked(
    stream: &mut TcpStream,
    entry: &FileEntry,
    offset: u64,
    status: &mut TransferStatus,
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
    let mut current_offset = offset;
    loop {
        let read = file.read(&mut buf).await?;
        if read == 0 {
            break;
        }
        write_packet(stream, PacketType::FileChunk, current_offset, &buf[..read]).await?;
        current_offset += read as u64;
        status.bytes_sent += read as u64;
    }
    Ok(())
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
