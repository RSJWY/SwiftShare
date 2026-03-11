mod discovery;
mod transport;
use anyhow::Result;
use transport::{start_listener, start_transfer, TransportHandle};
use std::sync::Arc;
use tokio::sync::Mutex;
use transport::{add_shared, clear_shared, fetch_remote_list, list_shared, pull_file, SharedEntry};
use tauri::Emitter;


#[tauri::command]
async fn start_transfer_command(paths: Vec<String>, target_ip: String, target_port: u16) -> Result<(), String> {
    let handle = TRANSPORT_HANDLE
        .get_or_init(|| Arc::new(Mutex::new(None)))
        .clone();
    let guard = handle.lock().await;
    let transport = guard.as_ref().ok_or_else(|| "Transport not initialized".to_string())?;
    start_transfer(transport, paths, target_ip, target_port)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn add_shared_command(paths: Vec<String>) -> Result<Vec<SharedEntry>, String> {
    let handle = TRANSPORT_HANDLE
        .get_or_init(|| Arc::new(Mutex::new(None)))
        .clone();
    let guard = handle.lock().await;
    let transport = guard.as_ref().ok_or_else(|| "Transport not initialized".to_string())?;
    add_shared(transport, paths).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_shared_command() -> Result<Vec<SharedEntry>, String> {
    let handle = TRANSPORT_HANDLE
        .get_or_init(|| Arc::new(Mutex::new(None)))
        .clone();
    let guard = handle.lock().await;
    let transport = guard.as_ref().ok_or_else(|| "Transport not initialized".to_string())?;
    list_shared(transport).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn clear_shared_command() -> Result<(), String> {
    let handle = TRANSPORT_HANDLE
        .get_or_init(|| Arc::new(Mutex::new(None)))
        .clone();
    let guard = handle.lock().await;
    let transport = guard.as_ref().ok_or_else(|| "Transport not initialized".to_string())?;
    clear_shared(transport).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn fetch_remote_list_command(target_ip: String, target_port: u16) -> Result<Vec<SharedEntry>, String> {
    let handle = TRANSPORT_HANDLE
        .get_or_init(|| Arc::new(Mutex::new(None)))
        .clone();
    let guard = handle.lock().await;
    let transport = guard.as_ref().ok_or_else(|| "Transport not initialized".to_string())?;
    fetch_remote_list(transport, target_ip, target_port)
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
    let handle = TRANSPORT_HANDLE
        .get_or_init(|| Arc::new(Mutex::new(None)))
        .clone();
    let guard = handle.lock().await;
    let transport = guard.as_ref().ok_or_else(|| "Transport not initialized".to_string())?;
    pull_file(transport, entry_id, target_ip, target_port, dest_dir, |progress| {
        let _ = app.emit("pull-progress", progress.clone());
    })
    .await
    .map_err(|e| e.to_string())
}

static TRANSPORT_HANDLE: std::sync::OnceLock<Arc<Mutex<Option<TransportHandle>>>> = std::sync::OnceLock::new();

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Ok(transport) = start_listener().await {
                    discovery::start(handle.clone(), transport.port).ok();
                    let storage = TRANSPORT_HANDLE
                        .get_or_init(|| Arc::new(Mutex::new(None)))
                        .clone();
                    let mut guard = storage.lock().await;
                    *guard = Some(transport);
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
            pull_file_command
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
