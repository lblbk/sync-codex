import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { syncToRemote, viewRemoteFile, testRemote } from "./ssh.js";
import { summarizeModels } from "./models.js";
import {
  findLocalCatalog,
  findLocalConfig,
  getStatus as getAutoSyncStatus,
  startAutoSync,
  stopAutoSync,
} from "./watcher.js";

export const PORT = 8001; // 生产服务器端口（8000 留给 Vite 开发服务器）
export const CC_SWITCH_API = "http://127.0.0.1:15721/v1/models";

const MAX_BODY_BYTES = 5 * 1024 * 1024; // 请求体上限 5MB
const UPSTREAM_TIMEOUT_MS = 8000;

const CORS_HEADERS = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
  "Access-Control-Allow-Headers": "Content-Type",
};

function sendJson(res, status, data, head = false) {
  res.writeHead(status, {
    "Content-Type": "application/json",
    ...CORS_HEADERS,
  });
  res.end(head ? undefined : JSON.stringify(data));
}

/** 代理本地 CC Switch 的模型列表接口 */
function proxyModels(req, res, head = false, summary = false) {
  const upstream = http.get(CC_SWITCH_API, { timeout: UPSTREAM_TIMEOUT_MS }, (upstreamRes) => {
    let data = "";
    upstreamRes.setEncoding("utf8");
    upstreamRes.on("data", (chunk) => (data += chunk));
    upstreamRes.on("end", () => {
      sendJson(
        res,
        upstreamRes.statusCode || 502,
        head
          ? {}
          : { models: summary ? summarizeModels(safeParseModels(data)) : safeParseModels(data) },
        head,
      );
    });
  });

  upstream.on("timeout", () => {
    upstream.destroy();
    sendJson(res, 504, { error: "CC Switch API 响应超时" });
  });

  upstream.on("error", () => {
    if (!res.headersSent) {
      sendJson(res, 502, { error: "无法连接到 CC Switch API" });
    }
  });
}

/** 解析上游返回的 JSON；解析失败时返回空列表，避免整个页面报错 */
function safeParseModels(raw) {
  try {
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed?.models) ? parsed.models : [];
  } catch {
    return [];
  }
}

function readBody(req, callback) {
  let body = "";
  let size = 0;
  let done = false;

  req.on("data", (chunk) => {
    if (done) return;
    size += chunk.length;
    if (size > MAX_BODY_BYTES) {
      done = true;
      callback(null, new Error("请求体过大"));
      return;
    }
    body += chunk.toString("utf8");
  });

  req.on("end", () => {
    if (done) return;
    done = true;
    try {
      callback(JSON.parse(body || "{}"));
    } catch (error) {
      callback(null, error);
    }
  });

  req.on("error", (error) => {
    if (!done) {
      done = true;
      callback(null, error);
    }
  });
}

export function handleApi(req, res) {
  for (const [key, value] of Object.entries(CORS_HEADERS)) {
    res.setHeader(key, value);
  }

  if (req.method === "OPTIONS") {
    res.writeHead(204);
    res.end();
    return;
  }

  const url = (req.url || "/").split("?")[0];

  if (url === "/api/models" && (req.method === "GET" || req.method === "HEAD")) {
    proxyModels(req, res, req.method === "HEAD");
    return;
  }

  if (url === "/api/models/summary" && req.method === "GET") {
    proxyModels(req, res, false, true);
    return;
  }

  if (url === "/api/sync" && req.method === "POST") {
    readBody(req, (params, error) => {
      if (error) {
        sendJson(res, 400, { success: false, error: error.message });
        return;
      }
      syncToRemote(params, (result) => {
        sendJson(res, result.success ? 200 : 500, result);
      });
    });
    return;
  }

  if (url === "/api/remote-view" && req.method === "POST") {
    readBody(req, (params, error) => {
      if (error) {
        sendJson(res, 400, { success: false, error: error.message });
        return;
      }
      viewRemoteFile(params, (result) => {
        sendJson(res, result.success ? 200 : 500, result);
      });
    });
    return;
  }

  if (url === "/api/remote-models" && req.method === "POST") {
    readBody(req, (params, error) => {
      if (error) {
        sendJson(res, 400, { success: false, error: error.message });
        return;
      }
      viewRemoteFile(params, (result) => {
        if (!result.success) {
          sendJson(res, 500, { success: false, error: result.error });
          return;
        }
        try {
          const parsed = JSON.parse(result.content);
          const models = Array.isArray(parsed)
            ? parsed
            : Array.isArray(parsed?.models)
              ? parsed.models
              : [];
          sendJson(res, 200, {
            success: true,
            models: summarizeModels(models),
          });
        } catch {
          sendJson(res, 500, {
            success: false,
            error: "远程文件不是有效的模型目录 JSON",
          });
        }
      });
    });
    return;
  }

  if (url === "/api/ssh-test" && req.method === "POST") {
    readBody(req, (params, error) => {
      if (error) {
        sendJson(res, 400, { success: false, error: error.message });
        return;
      }
      testRemote(params, (result) => {
        sendJson(res, result.success ? 200 : 500, result);
      });
    });
    return;
  }

  if (url === "/api/auto-sync/detect" && req.method === "GET") {
    const localFile = findLocalCatalog();
    sendJson(res, 200, {
      success: true,
      localFile,
      configFile: findLocalConfig(localFile),
    });
    return;
  }

  if (url === "/api/config/compare" && req.method === "POST") {
    readBody(req, (params, error) => {
      if (error) {
        sendJson(res, 400, { success: false, error: error.message });
        return;
      }

      const localConfigPath = findLocalConfig(params.localFile);
      const remoteConfigPath =
        params.remoteConfigPath ||
        (params.path
          ? path.join(path.dirname(params.path), "config.toml")
          : "");

      if (!remoteConfigPath) {
        sendJson(res, 400, {
          success: false,
          error: "缺少远程 config.toml 路径",
        });
        return;
      }

      let localContent = "";
      let localExists = false;
      try {
        localContent = fs.readFileSync(localConfigPath, "utf8");
        localExists = true;
      } catch {
        // 本地文件不存在时返回 exists:false
      }

      const local = {
        path: localConfigPath,
        exists: localExists,
        content: localContent,
      };

      viewRemoteFile(
        {
          host: params.host,
          user: params.user,
          password: params.password,
          path: remoteConfigPath,
        },
        (result) => {
          sendJson(res, 200, {
            success: true,
            local,
            remote: {
              path: remoteConfigPath,
              exists: result.success,
              content: result.success ? result.content : "",
              error: result.success ? "" : result.error,
            },
          });
        },
      );
    });
    return;
  }

  if (url === "/api/auto-sync/start" && req.method === "POST") {
    readBody(req, (params, error) => {
      if (error) {
        sendJson(res, 400, { success: false, error: error.message });
        return;
      }
      // 未传路径时由 watcher 自动使用默认目录（~/.codex 等）
      if (!params.localFile && !findLocalCatalog()) {
        sendJson(res, 400, { success: false, error: "缺少本地文件路径" });
        return;
      }
      try {
        sendJson(res, 200, { success: true, status: startAutoSync(params) });
      } catch (startErr) {
        sendJson(res, 400, { success: false, error: startErr.message });
      }
    });
    return;
  }

  if (url === "/api/auto-sync/stop" && req.method === "POST") {
    sendJson(res, 200, { success: true, status: stopAutoSync() });
    return;
  }

  if (url === "/api/auto-sync/status" && req.method === "GET") {
    sendJson(res, 200, { success: true, status: getAutoSyncStatus() });
    return;
  }

  sendJson(res, 404, { error: "Not Found" });
}

const server = http.createServer(handleApi);

function startApiServer(port = PORT, host = "127.0.0.1") {
  server.listen(port, host, () => {
    console.log(`[api] listening on http://${host}:${port}`);
  });
}

const isDirectRun =
  process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;

if (isDirectRun) {
  startApiServer();
}

export { startApiServer };
