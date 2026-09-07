import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { syncToRemote } from "./ssh.js";

let conf = null; // { host, user, path, password, localFile, intervalMs }
let timer = null;
let lastContent = null; // 上次成功读取的本地文件内容
let lastStat = null; // 上次 stat 结果（mtimeMs + size）
let syncingNow = false; // 防止并发同步

const status = {
  watching: false,
  localFile: "",
  fileExists: false,
  intervalMs: 2000,
  lastCheckAt: null,
  lastChangedAt: null,
  lastSyncAt: null,
  lastSyncOk: null,
  lastSyncMessage: "",
  totalSyncs: 0,
};

/** 在常见位置探测本地 CC Switch 模型目录文件 */
export function findLocalCatalog() {
  const candidates = [
    path.join(os.homedir(), ".codex", "cc-switch-model-catalog.json"),
    path.join(os.homedir(), ".cc-switch", "cc-switch-model-catalog.json"),
    process.env.USERPROFILE
      ? path.join(process.env.USERPROFILE, ".codex", "cc-switch-model-catalog.json")
      : "",
    process.env.APPDATA
      ? path.join(process.env.APPDATA, "cc-switch", "cc-switch-model-catalog.json")
      : "",
    process.env.LOCALAPPDATA
      ? path.join(process.env.LOCALAPPDATA, "cc-switch", "cc-switch-model-catalog.json")
      : "",
  ].filter(Boolean);
  return candidates.find((p) => fs.existsSync(p)) || candidates[0] || "";
}

/** config.toml 与模型目录文件放在同一目录 */
export function findLocalConfig(localCatalogPath) {
  const base = localCatalogPath || findLocalCatalog();
  return base ? path.join(path.dirname(base), "config.toml") : "";
}

export function startAutoSync(cfg) {
  stopAutoSync();
  const localFile = cfg.localFile || findLocalCatalog();
  if (!localFile) {
    throw new Error("未指定本地文件路径，且未探测到默认位置的模型目录文件");
  }
  conf = {
    host: cfg.host,
    user: cfg.user,
    path: cfg.path,
    password: cfg.password,
    localFile,
    intervalMs: Math.max(500, Number(cfg.intervalMs) || 2000),
  };
  Object.assign(status, {
    watching: true,
    localFile,
    fileExists: false,
    intervalMs: conf.intervalMs,
    lastCheckAt: null,
    lastChangedAt: null,
    lastSyncAt: null,
    lastSyncOk: null,
    lastSyncMessage: "",
    totalSyncs: 0,
  });
  // 启动后立即检查并同步一次，保证远程与本地一致
  checkOnce(true);
  timer = setInterval(() => checkOnce(false), conf.intervalMs);
  return getStatus();
}

export function stopAutoSync() {
  if (timer) {
    clearInterval(timer);
    timer = null;
  }
  conf = null;
  lastContent = null;
  lastStat = null;
  syncingNow = false;
  status.watching = false;
  return getStatus();
}

export function getStatus() {
  return { ...status, syncing: syncingNow };
}

/** 轮询检查：先用 stat 快检（mtime+size），没变化就不读文件内容，开销可忽略 */
function checkOnce(forceSync) {
  if (!conf) return;
  status.lastCheckAt = new Date().toISOString();

  let statEntry;
  try {
    statEntry = fs.statSync(conf.localFile);
    status.fileExists = true;
  } catch {
    status.fileExists = false;
    lastContent = null;
    lastStat = null;
    return;
  }

  const statChanged =
    !lastStat ||
    lastStat.mtimeMs !== statEntry.mtimeMs ||
    lastStat.size !== statEntry.size;
  if (!forceSync && !statChanged) return;
  lastStat = { mtimeMs: statEntry.mtimeMs, size: statEntry.size };

  let content = null;
  try {
    content = fs.readFileSync(conf.localFile, "utf8");
  } catch {
    return;
  }

  if (forceSync || content !== lastContent) {
    lastContent = content;
    status.lastChangedAt = new Date().toISOString();
    doSync(content);
  }
}

function doSync(content) {
  if (!conf || syncingNow) return;

  let parsed;
  try {
    parsed = JSON.parse(content);
  } catch {
    // 文件可能正在被写入，下一轮重试
    status.lastSyncMessage = "本地文件 JSON 解析失败，将自动重试";
    return;
  }

  syncingNow = true;
  syncToRemote(
    {
      host: conf.host,
      user: conf.user,
      path: conf.path,
      password: conf.password,
      // 原样上传整个文件内容（保留顶层结构，如 {models: [...]}），
      // 保证远程文件与本地文件完全一致
      raw: JSON.stringify(parsed, null, 2),
    },
    (result) => {
      syncingNow = false;
      status.lastSyncAt = new Date().toISOString();
      status.lastSyncOk = result.success;
      status.lastSyncMessage = result.success
        ? result.stdout || "同步成功"
        : result.error || "同步失败";
      if (result.success) {
        status.totalSyncs++;
      }
    },
  );
}
