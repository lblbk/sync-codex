use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::State;
use tokio::sync::Mutex;

use crate::models::{extract_models, summarize_models};
use crate::ssh;
use crate::watcher::{self, run_watcher, AutoConf, AutoWatcher};

const CC_SWITCH_API: &str = "http://127.0.0.1:15721/v1/models";

#[derive(Clone)]
pub struct AppState {
    pub watcher: Arc<Mutex<AutoWatcher>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            watcher: Arc::new(Mutex::new(AutoWatcher::new())),
        }
    }
}

async fn fetch_models() -> Result<Vec<Value>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(CC_SWITCH_API).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let text = resp.text().await.map_err(|e| e.to_string())?;
    Ok(extract_models(&text))
}

#[tauri::command]
pub async fn health() -> Result<Value, String> {
    Ok(json!({ "success": true }))
}

#[tauri::command]
pub async fn models() -> Result<Value, String> {
    Ok(json!({ "models": fetch_models().await? }))
}

#[tauri::command]
pub async fn model_summary() -> Result<Value, String> {
    Ok(json!({ "models": summarize_models(Some(&json!(fetch_models().await?))) }))
}

#[tauri::command]
pub async fn sync(body: Value) -> Result<Value, String> {
    let host = get_str(&body, "host");
    let user = get_str(&body, "user");
    let password = get_str(&body, "password");
    let path = get_str(&body, "path");
    let raw = body
        .get("raw")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            body.get("models")
                .map(|models| serde_json::to_string_pretty(models).unwrap_or_default())
                .unwrap_or_else(|| "[]".to_string())
        });

    let res = ssh::sync_remote(&host, &user, &password, &path, &raw).await;

    match res {
        Ok(stdout) => Ok(json!({ "success": true, "error": "", "stdout": stdout })),
        Err(error) => Err(format!("上传失败: {error}")),
    }
}

#[tauri::command]
pub async fn remote_view(body: Value) -> Result<Value, String> {
    let host = get_str(&body, "host");
    let user = get_str(&body, "user");
    let password = get_str(&body, "password");
    let path = get_str(&body, "path");

    let res = ssh::view_remote_file(&host, &user, &password, &path).await;

    match res {
        Ok(content) => Ok(json!({ "success": true, "error": "", "content": content })),
        Err(error) => Err(error),
    }
}

#[tauri::command]
pub async fn remote_models(body: Value) -> Result<Value, String> {
    let host = get_str(&body, "host");
    let user = get_str(&body, "user");
    let password = get_str(&body, "password");
    let path = get_str(&body, "path");

    let res = ssh::view_remote_file(&host, &user, &password, &path).await;

    match res {
        Ok(content) => {
            let raw_models = json!(extract_models(&content));
            Ok(json!({
                "success": true,
                "models": summarize_models(Some(&raw_models))
            }))
        }
        Err(error) => Err(error),
    }
}

#[tauri::command]
pub async fn ssh_test(body: Value) -> Result<Value, String> {
    let host = get_str(&body, "host");
    let user = get_str(&body, "user");
    let password = get_str(&body, "password");

    let res = ssh::test_remote(&host, &user, &password).await;

    match res {
        Ok(home) => Ok(json!({ "success": true, "error": "", "home": home })),
        Err(error) => Err(error),
    }
}

#[tauri::command]
pub async fn auto_detect() -> Result<Value, String> {
    let local_file = watcher::find_local_catalog();
    let config_file = watcher::find_local_config(&local_file);
    Ok(json!({ "success": true, "localFile": local_file, "configFile": config_file }))
}

#[tauri::command]
pub async fn config_compare(body: Value) -> Result<Value, String> {
    let local_file = get_str(&body, "localFile");
    let local_path = if local_file.is_empty() {
        watcher::find_local_catalog()
    } else {
        local_file.clone()
    };
    let config_path = if local_path.is_empty() {
        String::new()
    } else {
        watcher::find_local_config(&local_path)
    };
    let remote_config = get_str(&body, "remoteConfigPath");
    let remote_config = if remote_config.is_empty() {
        let base_path = get_str(&body, "path");
        Path::new(&base_path)
            .parent()
            .map(|p| p.join("config.toml").to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        remote_config
    };
    let host = get_str(&body, "host");
    let user = get_str(&body, "user");
    let password = get_str(&body, "password");

    let local_content = std::fs::read_to_string(&config_path).ok();
    let remote = ssh::view_remote_file(&host, &user, &password, &remote_config).await;
    let remote_exists = remote.is_ok();
    let remote_content = remote.clone().unwrap_or_default();
    let remote_error = remote.err().unwrap_or_default();

    Ok(json!({
        "success": true,
        "local": {
            "path": config_path,
            "exists": local_content.is_some(),
            "content": local_content.unwrap_or_default()
        },
        "remote": {
            "path": remote_config,
            "exists": remote_exists,
            "content": remote_content,
            "error": remote_error
        }
    }))
}

#[tauri::command]
pub async fn auto_sync_start(state: State<'_, AppState>, body: Value) -> Result<Value, String> {
    let local_file = get_str(&body, "localFile");
    let local_file = if local_file.is_empty() {
        watcher::find_local_catalog()
    } else {
        local_file
    };
    let cfg = AutoConf {
        host: get_str(&body, "host"),
        user: get_str(&body, "user"),
        password: get_str(&body, "password"),
        path: get_str(&body, "path"),
        local_file,
        interval_ms: body
            .get("intervalMs")
            .and_then(Value::as_u64)
            .unwrap_or(2000)
            .max(500),
    };

    let mut w = state.watcher.lock().await;
    match w.configure(cfg) {
        Ok(_status) => {
            let watcher = state.watcher.clone();
            w.attach_task(tokio::spawn(run_watcher(watcher)));
            let status = w.status();
            Ok(json!({ "success": true, "status": status_to_json(status) }))
        }
        Err(error) => Err(error),
    }
}

#[tauri::command]
pub async fn auto_sync_stop(state: State<'_, AppState>) -> Result<Value, String> {
    let mut w = state.watcher.lock().await;
    Ok(json!({ "success": true, "status": status_to_json(w.stop()) }))
}

#[tauri::command]
pub async fn auto_sync_status(state: State<'_, AppState>) -> Result<Value, String> {
    let w = state.watcher.lock().await;
    Ok(json!({ "success": true, "status": status_to_json(w.status()) }))
}

fn status_to_json(status: watcher::AutoStatus) -> Value {
    json!({
        "watching": status.watching,
        "localFile": status.local_file,
        "fileExists": status.file_exists,
        "intervalMs": status.interval_ms,
        "lastCheckAt": status.last_check_at,
        "lastChangedAt": status.last_changed_at,
        "lastSyncAt": status.last_sync_at,
        "lastSyncOk": status.last_sync_ok,
        "lastSyncMessage": status.last_sync_message,
        "totalSyncs": status.total_syncs,
        "syncing": status.syncing
    })
}

fn get_str(body: &Value, key: &str) -> String {
    body.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}
