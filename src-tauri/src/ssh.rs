use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

use ssh2::{FileStat, Session, Sftp};

pub const VIEW_MAX_BYTES: u64 = 2 * 1024 * 1024;

fn connect(host: &str, user: &str, password: &str) -> Result<Session, String> {
    let addr = (host, 22);
    let tcp = TcpStream::connect(addr).map_err(|e| format!("TCP 连接失败: {e}"))?;
    tcp.set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    tcp.set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;

    let mut session = Session::new().map_err(|e| e.to_string())?;
    session.set_tcp_stream(tcp);
    session.handshake().map_err(|e| format!("SSH 握手失败: {e}"))?;
    session
        .userauth_password(user, password)
        .map_err(|e| format!("SSH 认证失败: {e}"))?;
    if !session.authenticated() {
        return Err("SSH 认证未通过".to_string());
    }
    Ok(session)
}

fn with_sftp(
    host: &str,
    user: &str,
    password: &str,
    work: impl FnOnce(&mut Sftp) -> Result<(), String>,
) -> Result<(), String> {
    let session = connect(host, user, password)?;
    let mut sftp = session.sftp().map_err(|e| format!("SFTP 初始化失败: {e}"))?;
    work(&mut sftp)
}

pub fn test_remote(host: &str, user: &str, password: &str) -> Result<(), String> {
    let session = connect(host, user, password)?;
    session.sftp().map_err(|e| format!("SFTP 初始化失败: {e}"))?;
    Ok(())
}

pub fn sync_remote(host: &str, user: &str, password: &str, remote_path: &str, raw: &str) -> Result<String, String> {
    with_sftp(host, user, password, |sftp| {
        let mut file = sftp
            .create(Path::new(remote_path))
            .map_err(|e| format!("创建远程文件失败: {e}"))?;
        let stat = FileStat {
            size: None,
            uid: None,
            gid: None,
            perm: Some(0o644),
            atime: None,
            mtime: None,
        };
        sftp.setstat(Path::new(remote_path), stat)
            .map_err(|e| e.to_string())?;
        file.write_all(raw.as_bytes()).map_err(|e| format!("写入远程文件失败: {e}"))?;
        Ok(())
    })?;
    Ok(format!("文件已上传到 {remote_path}"))
}

pub fn view_remote_file(host: &str, user: &str, password: &str, remote_path: &str) -> Result<String, String> {
    let session = connect(host, user, password)?;
    let sftp = session.sftp().map_err(|e| format!("SFTP 初始化失败: {e}"))?;
    let stat = sftp
        .stat(Path::new(remote_path))
        .map_err(|e| format!("读取远程文件状态失败: {e}"))?;
    if stat.is_dir() {
        return Err("目标路径是一个目录".to_string());
    }
    if stat.size.unwrap_or(0) > VIEW_MAX_BYTES {
        return Err(format!(
            "文件过大（{} 字节），仅支持预览前 {}KB",
            stat.size.unwrap_or(0),
            VIEW_MAX_BYTES / 1024
        ));
    }

    let mut file = sftp
        .open(Path::new(remote_path))
        .map_err(|e| format!("打开远程文件失败: {e}"))?;
    let mut content = Vec::new();
    file.read_to_end(&mut content)
        .map_err(|e| format!("读取远程文件失败: {e}"))?;
    String::from_utf8(content).map_err(|_| "远程文件不是 UTF-8 文本".to_string())
}
