use std::sync::Arc;
use std::time::Duration;

use russh::client::{self, Handle};
use russh::keys::PublicKeyOrCertificate;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::FileAttributes;
use tokio::io::AsyncWriteExt;

pub const VIEW_MAX_BYTES: u64 = 2 * 1024 * 1024;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_FILE_MODE: u32 = 0o644;

struct Client;

impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        // The previous ssh2 implementation did not pin host keys either, so keep
        // the same trust model: accept the server key. The channel is still
        // encrypted; this only skips known_hosts / MITM pinning.
        Ok(true)
    }
}

async fn connect(host: &str, user: &str, password: &str) -> Result<Handle<Client>, String> {
    let config = Arc::new(client::Config::default());
    let mut handle = tokio::time::timeout(
        CONNECT_TIMEOUT,
        client::connect(config, (host, 22), Client),
    )
    .await
    .map_err(|_| "SSH 连接超时".to_string())?
    .map_err(|e| format!("SSH 连接失败: {e}"))?;

    let auth = handle
        .authenticate_password(user, password)
        .await
        .map_err(|e| format!("SSH 认证失败: {e}"))?;
    if !auth.success() {
        return Err("SSH 认证未通过".to_string());
    }
    Ok(handle)
}

/// Connect, authenticate, open a session channel and start the SFTP subsystem.
/// The returned handle must stay alive while `SftpSession` is in use.
async fn open_sftp(
    host: &str,
    user: &str,
    password: &str,
) -> Result<(Handle<Client>, SftpSession), String> {
    let handle = connect(host, user, password).await?;
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| format!("打开 SSH 通道失败: {e}"))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| format!("启动 SFTP 子系统失败: {e}"))?;
    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| format!("SFTP 初始化失败: {e}"))?;
    Ok((handle, sftp))
}

/// Connect and return the remote user's home directory (SFTP REALPATH of `.`),
/// which the UI uses to derive `~/.codex/...` paths automatically.
pub async fn test_remote(host: &str, user: &str, password: &str) -> Result<String, String> {
    let (_handle, sftp) = open_sftp(host, user, password).await?;
    sftp.canonicalize(".")
        .await
        .map_err(|e| format!("获取远程用户目录失败: {e}"))
}

pub async fn sync_remote(
    host: &str,
    user: &str,
    password: &str,
    remote_path: &str,
    raw: &str,
) -> Result<String, String> {
    let (_handle, sftp) = open_sftp(host, user, password).await?;

    // `create` = CREATE | TRUNCATE | WRITE, matching the old ssh2 `sftp.create`.
    let mut file = sftp
        .create(remote_path)
        .await
        .map_err(|e| format!("创建远程文件失败: {e}"))?;
    file.write_all(raw.as_bytes())
        .await
        .map_err(|e| format!("写入远程文件失败: {e}"))?;
    file.flush()
        .await
        .map_err(|e| format!("刷新远程文件失败: {e}"))?;
    file.close()
        .await
        .map_err(|e| format!("关闭远程文件失败: {e}"))?;

    let attrs = FileAttributes {
        permissions: Some(DEFAULT_FILE_MODE),
        ..Default::default()
    };
    sftp.set_metadata(remote_path, attrs)
        .await
        .map_err(|e| format!("设置远程文件权限失败: {e}"))?;

    Ok(format!("文件已上传到 {remote_path}"))
}

pub async fn view_remote_file(
    host: &str,
    user: &str,
    password: &str,
    remote_path: &str,
) -> Result<String, String> {
    let (_handle, sftp) = open_sftp(host, user, password).await?;

    let stat = sftp
        .metadata(remote_path)
        .await
        .map_err(|e| format!("读取远程文件状态失败: {e}"))?;
    if stat.is_dir() {
        return Err("目标路径是一个目录".to_string());
    }
    let size = stat.len();
    if size > VIEW_MAX_BYTES {
        return Err(format!(
            "文件过大（{size} 字节），仅支持预览前 {}KB",
            VIEW_MAX_BYTES / 1024
        ));
    }

    let content = sftp
        .read(remote_path)
        .await
        .map_err(|e| format!("读取远程文件失败: {e}"))?;
    String::from_utf8(content).map_err(|_| "远程文件不是 UTF-8 文本".to_string())
}
