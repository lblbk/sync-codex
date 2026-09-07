import { Client } from "ssh2";

const CONNECT_TIMEOUT_MS = 10000;
const VIEW_MAX_BYTES = 2 * 1024 * 1024; // 远程文件读取上限 2MB

/**
 * 建立 SSH 连接并进入 SFTP 会话。
 * 无论成功失败，都保证 callback 恰好调用一次：
 *   callback(err, data) —— err 为 null 时 data 是 work 的产出
 */
function withSftp({ host, user, password }, work, callback) {
  const conn = new Client();
  let settled = false;

  const done = (err, data) => {
    if (settled) return;
    settled = true;
    try {
      conn.end();
    } catch {
      // 连接可能已断开，忽略清理异常
    }
    callback(err, data);
  };

  conn.on("ready", () => {
    conn.sftp((err, sftp) => {
      if (err) {
        done(err);
        return;
      }
      try {
        work(sftp, done);
      } catch (error) {
        done(error);
      }
    });
  });

  conn.on("error", (err) => done(err));

  conn.connect({
    host,
    port: 22,
    username: user,
    password,
    readyTimeout: CONNECT_TIMEOUT_MS,
  });
}

/** 把模型列表以 JSON 形式流式写入远程文件 */
export function syncToRemote(params, callback) {
  const { host, user, path: remotePath, password } = params;
  // raw 优先：原样写入（保留顶层结构）；否则序列化 models 数组
  const jsonContent =
    params.raw !== undefined
      ? params.raw
      : JSON.stringify(params.models ?? [], null, 2);

  withSftp(
    { host, user, password },
    (sftp, done) => {
      const stream = sftp.createWriteStream(remotePath, {
        flags: "w",
        mode: 0o644,
        autoClose: true,
      });

      let writeError = null;
      stream.on("error", (error) => {
        writeError = error;
      });
      stream.on("close", () => {
        if (writeError) {
          done(writeError);
          return;
        }
        done(null, { stdout: "文件已上传到 " + remotePath });
      });

      stream.end(jsonContent, "utf8");
    },
    (err, data) => {
      if (err) {
        callback({ success: false, error: "上传失败: " + err.message });
        return;
      }
      callback({ success: true, error: "", stdout: data.stdout });
    },
  );
}

/** 读取远程文件内容（有大小上限，避免把大文件整读进内存） */
export function viewRemoteFile(params, callback) {
  const { host, user, path: remotePath, password } = params;

  withSftp(
    { host, user, password },
    (sftp, done) => {
      sftp.stat(remotePath, (statErr, stats) => {
        if (statErr) {
          done(statErr);
          return;
        }
        if (stats.isDirectory()) {
          done(new Error("目标路径是一个目录"));
          return;
        }
        if (stats.size > VIEW_MAX_BYTES) {
          done(
            new Error(
              `文件过大（${stats.size} 字节），仅支持预览前 ${Math.floor(VIEW_MAX_BYTES / 1024)}KB`,
            ),
          );
          return;
        }
        sftp.readFile(remotePath, { encoding: "utf8" }, (readErr, content) => {
          if (readErr) {
            done(readErr);
            return;
          }
          done(null, { content });
        });
      });
    },
    (err, data) => {
      if (err) {
        callback({ success: false, error: err.message });
        return;
      }
      callback({ success: true, error: "", content: data.content });
    },
  );
}

/** 测试 SSH 连通性：成功建立 SFTP 会话即视为连通 */
export function testRemote(params, callback) {
  const { host, user, password } = params;
  withSftp(
    { host, user, password },
    (sftp, done) => done(null, {}),
    (err) => {
      if (err) {
        callback({ success: false, error: err.message });
        return;
      }
      callback({ success: true, error: "" });
    },
  );
}
