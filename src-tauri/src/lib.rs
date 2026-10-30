mod api;
mod models;
mod ssh;
mod watcher;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(api::AppState::new())
        .invoke_handler(tauri::generate_handler![
            api::health,
            api::models,
            api::model_summary,
            api::sync,
            api::remote_view,
            api::remote_models,
            api::ssh_test,
            api::auto_detect,
            api::config_compare,
            api::auto_sync_start,
            api::auto_sync_stop,
            api::auto_sync_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
