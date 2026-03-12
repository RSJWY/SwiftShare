mod discovery;
mod transport;
use anyhow::Result;
use transport::{start_listener, start_transfer, TransportHandle};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, Semaphore};
use transport::{add_shared, clear_shared, fetch_remote_list, list_shared, pull_file, SharedEntry};
use tauri::{Emitter, Manager, WindowEvent};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use tokio::fs;

/// Wait for TRANSPORT_HANDLE to be initialized (up to 5 seconds).
/// All commands should call this instead of manually checking Option.
async fn wait_transport() -> Result<TransportHandle, String> {
    let handle = TRANSPORT_HANDLE
        .get_or_init(|| Arc::new(Mutex::new(None)))
        .clone();
    for _ in 0..50 {
        let guard = handle.lock().await;
        if let Some(t) = guard.as_ref() {
            return Ok(t.clone());
        }
        drop(guard);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Err("Transport not initialized after 5s".to_string())
}

#[tauri::command]
async fn start_transfer_command(paths: Vec<String>, target_ip: String, target_port: u16) -> Result<(), String> {
    let settings = SETTINGS_STATE
        .get_or_init(|| Arc::new(SettingsState::new()))
        .clone();
    let send_limit = { settings.send_limit.read().await.clone() };
    let _permit = send_limit.acquire().await.map_err(|e| e.to_string())?;
    let transport = wait_transport().await?;
    let max_mbps = settings.max_mbps.load(Ordering::Relaxed);
    start_transfer(&transport, paths, target_ip, target_port, max_mbps)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn add_shared_command(paths: Vec<String>) -> Result<Vec<SharedEntry>, String> {
    let transport = wait_transport().await?;
    add_shared(&transport, paths).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_shared_command() -> Result<Vec<SharedEntry>, String> {
    let transport = wait_transport().await?;
    list_shared(&transport).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn clear_shared_command() -> Result<(), String> {
    let transport = wait_transport().await?;
    clear_shared(&transport).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn fetch_remote_list_command(target_ip: String, target_port: u16) -> Result<Vec<SharedEntry>, String> {
    let transport = wait_transport().await?;
    fetch_remote_list(&transport, target_ip, target_port)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn pull_file_command(
    app: tauri::AppHandle,
    entry_id: String,
    target_ip: String,
    target_port: u16,
    dest_dir: String,
) -> Result<(), String> {
    let settings = SETTINGS_STATE
        .get_or_init(|| Arc::new(SettingsState::new()))
        .clone();
    let pull_limit = { settings.pull_limit.read().await.clone() };
    let _permit = pull_limit.acquire().await.map_err(|e| e.to_string())?;
    let transport = wait_transport().await?;
    let max_mbps = settings.max_mbps.load(Ordering::Relaxed);
    pull_file(&transport, entry_id, target_ip, target_port, dest_dir, max_mbps, |progress| {
        let _ = app.emit("pull-progress", progress.clone());
    })
    .await
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn pull_to_temp_command(
    app: tauri::AppHandle,
    entry_id: String,
    target_ip: String,
    target_port: u16,
) -> Result<String, String> {
    let temp_root: std::path::PathBuf = app
        .path()
        .temp_dir()
        .map_err(|e: tauri::Error| e.to_string())?;
    let cache_dir = temp_root.join("swiftshare-cache");
    fs::create_dir_all(&cache_dir)
        .await
        .map_err(|e: std::io::Error| e.to_string())?;
    let settings = SETTINGS_STATE
        .get_or_init(|| Arc::new(SettingsState::new()))
        .clone();
    let pull_limit = { settings.pull_limit.read().await.clone() };
    let _permit = pull_limit.acquire().await.map_err(|e| e.to_string())?;
    let transport = wait_transport().await?;
    let max_mbps = settings.max_mbps.load(Ordering::Relaxed);
    let pulled_name = pull_file(
        &transport,
        entry_id.clone(),
        target_ip,
        target_port,
        cache_dir.to_string_lossy().to_string(),
        max_mbps,
        |progress| {
        let _ = app.emit("pull-progress", progress.clone());
    },
    )
    .await
    .map_err(|e| e.to_string())?;

    let resolved = cache_dir.join(pulled_name);
    Ok(resolved.to_string_lossy().to_string())
}

#[tauri::command]
async fn get_local_machine_id_command() -> Result<String, String> {
    let transport = wait_transport().await?;
    Ok(transport.machine_id.clone())
}

#[tauri::command]
async fn update_settings_command(
    max_concurrent: u32,
    max_mbps: u64,
    discovery_interval_ms: u64,
    same_subnet_only: bool,
) -> Result<(), String> {
    let settings = SETTINGS_STATE
        .get_or_init(|| Arc::new(SettingsState::new()))
        .clone();
    let max_concurrent = max_concurrent.clamp(1, 8) as usize;
    {
        let mut send_limit = settings.send_limit.write().await;
        *send_limit = Arc::new(Semaphore::new(max_concurrent));
    }
    {
        let mut pull_limit = settings.pull_limit.write().await;
        *pull_limit = Arc::new(Semaphore::new(max_concurrent));
    }
    settings.max_mbps.store(max_mbps, Ordering::Relaxed);
    settings
        .discovery_interval_ms
        .store(discovery_interval_ms.max(1_000), Ordering::Relaxed);
    settings
        .same_subnet_only
        .store(if same_subnet_only { 1 } else { 0 }, Ordering::Relaxed);
    Ok(())
}

static TRANSPORT_HANDLE: std::sync::OnceLock<Arc<Mutex<Option<TransportHandle>>>> = std::sync::OnceLock::new();
static SETTINGS_STATE: std::sync::OnceLock<Arc<SettingsState>> = std::sync::OnceLock::new();

#[derive(Debug)]
struct SettingsState {
    send_limit: RwLock<Arc<Semaphore>>,
    pull_limit: RwLock<Arc<Semaphore>>,
    max_mbps: AtomicU64,
    discovery_interval_ms: AtomicU64,
    same_subnet_only: AtomicUsize,
}

impl SettingsState {
    fn new() -> Self {
        Self {
            send_limit: RwLock::new(Arc::new(Semaphore::new(2))),
            pull_limit: RwLock::new(Arc::new(Semaphore::new(2))),
            max_mbps: AtomicU64::new(0),
            discovery_interval_ms: AtomicU64::new(5_000),
            same_subnet_only: AtomicUsize::new(0),
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let settings = SETTINGS_STATE
        .get_or_init(|| Arc::new(SettingsState::new()))
        .clone();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_drag::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let cleanup_handle = app.handle().clone();
            if let Some(window) = app.get_webview_window("main") {
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { .. } = event {
                        let cleanup_handle = cleanup_handle.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Ok(temp_root) = cleanup_handle.path().temp_dir() {
                                let cache_dir = temp_root.join("swiftshare-cache");
                                let _ = tokio::fs::remove_dir_all(cache_dir).await;
                            }
                        });
                    }
                });
            }
            tauri::async_runtime::spawn(async move {
                if let Ok(transport) = start_listener().await {
                    let transport = Arc::new(transport);
                    discovery::start(handle.clone(), transport.clone(), settings.clone()).ok();
                    let storage = TRANSPORT_HANDLE
                        .get_or_init(|| Arc::new(Mutex::new(None)))
                        .clone();
                    let mut guard = storage.lock().await;
                    *guard = Some((*transport).clone());
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_transfer_command,
            add_shared_command,
            list_shared_command,
            clear_shared_command,
            fetch_remote_list_command,
            pull_file_command,
            pull_to_temp_command,
            get_local_machine_id_command,
            update_settings_command
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
