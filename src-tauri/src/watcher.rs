use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::ssh;

#[derive(Clone)]
pub struct AutoConf {
    pub host: String,
    pub user: String,
    pub password: String,
    pub path: String,
    pub local_file: String,
    pub interval_ms: u64,
}

#[derive(Clone)]
pub struct AutoStatus {
    pub watching: bool,
    pub local_file: String,
    pub file_exists: bool,
    pub interval_ms: u64,
    pub last_check_at: Option<u64>,
    pub last_changed_at: Option<u64>,
    pub last_sync_at: Option<u64>,
    pub last_sync_ok: Option<bool>,
    pub last_sync_message: String,
    pub total_syncs: u64,
    pub syncing: bool,
}

impl Default for AutoStatus {
    fn default() -> Self {
        Self {
            watching: false,
            local_file: String::new(),
            file_exists: false,
            interval_ms: 2000,
            last_check_at: None,
            last_changed_at: None,
            last_sync_at: None,
            last_sync_ok: None,
            last_sync_message: String::new(),
            total_syncs: 0,
            syncing: false,
        }
    }
}

pub struct AutoWatcher {
    conf: Option<AutoConf>,
    status: AutoStatus,
    task: Option<JoinHandle<()>>,
}

pub fn now_ms() -> u64 {
    let now = std::time::SystemTime::now();
    now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn find_local_catalog() -> String {
    let mut candidates = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".codex").join("cc-switch-model-catalog.json"));
        candidates.push(home.join(".cc-switch").join("cc-switch-model-catalog.json"));
    }
    if let Some(p) = env::var_os("USERPROFILE") {
        candidates.push(PathBuf::from(p).join(".codex").join("cc-switch-model-catalog.json"));
    }
    if let Some(p) = env::var_os("APPDATA") {
        candidates.push(PathBuf::from(p).join("cc-switch").join("cc-switch-model-catalog.json"));
    }
    if let Some(p) = env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(p).join("cc-switch").join("cc-switch-model-catalog.json"));
    }

    candidates
        .into_iter()
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn find_local_config(catalog_path: &str) -> String {
    if catalog_path.is_empty() {
        return String::new();
    }
    Path::new(catalog_path)
        .parent()
        .map(|p| p.join("config.toml").to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl AutoWatcher {
    pub fn new() -> Self {
        Self {
            conf: None,
            status: AutoStatus::default(),
            task: None,
        }
    }

    pub fn configure(&mut self, cfg: AutoConf) -> Result<AutoStatus, String> {
        if cfg.local_file.is_empty() {
            return Err("未指定本地文件路径，且未探测到默认位置的模型目录文件".to_string());
        }
        self.conf = Some(cfg);
        self.status = AutoStatus {
            watching: true,
            local_file: self.conf.as_ref().unwrap().local_file.clone(),
            file_exists: false,
            interval_ms: self.conf.as_ref().unwrap().interval_ms,
            ..Default::default()
        };
        Ok(self.status.clone())
    }

    pub fn stop(&mut self) -> AutoStatus {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.conf = None;
        self.status = AutoStatus::default();
        self.status.clone()
    }

    pub fn status(&self) -> AutoStatus {
        self.status.clone()
    }

    pub fn attach_task(&mut self, handle: JoinHandle<()>) {
        self.task = Some(handle);
    }

    pub async fn run(&mut self, force: bool) {
        let Some(conf) = self.conf.clone() else {
            return;
        };
        self.status.last_check_at = Some(now_ms());

        let Ok(meta) = fs::metadata(&conf.local_file) else {
            self.status.file_exists = false;
            return;
        };
        self.status.file_exists = true;

        let changed_at = meta
            .modified()
            .ok()
            .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64)
            .unwrap_or(0);
        if self.status.last_changed_at == Some(changed_at) && !force {
            return;
        }
        self.status.last_changed_at = Some(changed_at);

        let Ok(content) = fs::read_to_string(&conf.local_file) else {
            return;
        };
        let Ok(parsed) = serde_json::from_str::<Value>(&content) else {
            self.status.last_sync_message = "本地文件 JSON 解析失败，将自动重试".to_string();
            return;
        };
        let raw = serde_json::to_string_pretty(&parsed).unwrap_or(content);

        if self.status.syncing {
            return;
        }
        self.status.syncing = true;
        let result = ssh::sync_remote(&conf.host, &conf.user, &conf.password, &conf.path, &raw);
        self.status.syncing = false;
        self.status.last_sync_at = Some(now_ms());
        self.status.last_sync_ok = Some(result.is_ok());
        self.status.last_sync_message = result
            .as_ref()
            .map(|s| s.clone())
            .unwrap_or_else(|e| e.clone());
        if result.is_ok() {
            self.status.total_syncs += 1;
        }
    }
}

pub(crate) async fn run_watcher(shared: Arc<Mutex<AutoWatcher>>) {
    let interval = {
        let w = shared.lock().await;
        w.conf.as_ref().map(|c| c.interval_ms).unwrap_or(2000).max(500)
    };
    let mut force = true;
    loop {
        {
            let mut w = shared.lock().await;
            w.run(force).await;
        }
        force = false;
        tokio::time::sleep(Duration::from_millis(interval)).await;
    }
}
