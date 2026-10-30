<script>
  import { onDestroy, onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { backend } from "./api.js";
  import ModelList from "./ModelList.svelte";
  import LogTable from "./LogTable.svelte";

  const STORAGE_KEY = "codex_sync_config";
  const LOG_KEY = "codex_sync_logs";
  const SYNC_META_KEY = "codex_sync_meta";

  const DEFAULTS = {
    remoteHost: "192.168.31.165",
    remoteUser: "humble",
    remotePath: "/home/humble/.codex/cc-switch-model-catalog.json",
    sshPassword: "",
  };

  const SSH_TEXT = { unknown: "未测试", ok: "已连接", fail: "失败" };
  const CC_TEXT = { unknown: "未测试", ok: "在线", fail: "离线" };
  const TYPE_TEXT = {
    sync: "手动同步",
    auto: "自动同步",
    connection: "连接",
    compare: "对比",
    system: "系统",
  };

  // ---------- 连接配置 ----------
  let remoteHost = $state(DEFAULTS.remoteHost);
  let remoteUser = $state(DEFAULTS.remoteUser);
  let remotePath = $state(DEFAULTS.remotePath);
  let sshPassword = $state(DEFAULTS.sshPassword);
  let sshState = $state("unknown"); // unknown | ok | fail
  let ccState = $state("unknown");
  let testing = $state(false);
  let serverOnline = $state(true);
  let healthTimer = null;
  let unlistenWindowResize = null;
  let isMaximized = $state(false);

  // ---------- 手动同步 ----------
  let syncing = $state(false);
  let lastManualSyncAt = $state(0);
  let lastManualSyncOk = $state(null);
  let lastAutoSyncAt = $state(0);
  let lastAutoSyncOk = $state(null);
  let lastStatusAt = $state(0);

  // ---------- 自动同步 ----------
  let autoOn = $state(false);
  let localFile = $state("");
  let intervalMs = $state(2000);
  let autoStatus = $state(null);
  let autoBusy = $state(false);
  let pollTimer = null;
  let detecting = $state(false);
  let detectState = $state("idle");
  let detectMessage = $state("");
  let statusFeedback = $state("");

  // ---------- 调试（远程对比） ----------
  let remoteOpen = $state(false);
  let remoteContent = $state("");
  let remoteError = $state("");
  let remoteParsed = $state(null);
  let loading = $state(false);
  let compareResult = $state(null);
  let localModels = $state([]);
  let showDiff = $state(false);
  let configCompare = $state(null);

  // ---------- 日志 ----------
  let logs = $state([]);
  let statsMode = $state("7d"); // 7d | 30d
  let hoverStatDate = $state("");
  let selectedStatDate = $state("");

  let autoRefreshAt = $derived.by(() => {
    const server = autoStatus?.lastCheckAt
      ? new Date(autoStatus.lastCheckAt).getTime()
      : 0;
    return Math.max(server || 0, lastStatusAt || 0);
  });
  let autoSyncAt = $derived(
    autoStatus?.lastSyncAt
      ? new Date(autoStatus.lastSyncAt).getTime()
      : lastAutoSyncAt,
  );
  let autoSyncOkShown = $derived(
    autoStatus?.lastSyncAt ? autoStatus.lastSyncOk : lastAutoSyncOk,
  );

  // 面板状态色：ok=已同步，bad=未同步，空=未对比
  let panelState = $derived(
    compareResult === null ? "" : compareResult.synced ? "ok" : "bad",
  );

  // 源文件对比：两侧规范化缩进后逐行 LCS diff
  let remoteNormText = $derived(
    remoteParsed ? JSON.stringify(remoteParsed, null, 2) : "",
  );
  let localNormText = $derived(
    localModels.length ? JSON.stringify(localModels, null, 2) : "",
  );
  let diffRows = $derived(
    diffLines(remoteNormText.split("\n"), localNormText.split("\n")),
  );

  const remoteConfigPath = $derived(
    remotePath.replace(/[^/\\]+$/, "config.toml"),
  );

  // 同步次数统计：按本地日期汇总，用于柱状图展示
  let syncStats = $derived.by(() => {
    const dayCount = statsMode === "30d" ? 30 : 7;
    const start = new Date();
    start.setHours(0, 0, 0, 0);
    start.setDate(start.getDate() - dayCount + 1);

    const buckets = new Map();
    for (let index = 0; index < dayCount; index++) {
      const date = new Date(start);
      date.setDate(start.getDate() + index);
      const key = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
      buckets.set(key, {
        date: key,
        label: `${date.getMonth() + 1}/${date.getDate()}`,
        index,
        ok: 0,
        fail: 0,
        total: 0,
      });
    }

    const arr = logs.filter(
      (l) =>
        l.ts >= start.getTime() &&
        ((l.type === "sync" &&
          (l.message.includes("同步成功") || l.message.includes("同步失败"))) ||
          (l.type === "auto" &&
            (l.level === "success" || l.level === "error"))),
    );

    for (const log of arr) {
      const date = new Date(log.ts);
      const key = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
      const bucket = buckets.get(key);
      if (!bucket) continue;
      bucket.total++;
      if (log.level === "success") bucket.ok++;
      if (log.level === "error") bucket.fail++;
    }

    const days = Array.from(buckets.values()).map((day) => ({
      ...day,
      showLabel: dayCount === 7 || day.index % 5 === 0,
    }));

    return {
      total: arr.length,
      ok: arr.filter((l) => l.level === "success").length,
      fail: arr.filter((l) => l.level === "error").length,
      days,
      max: Math.max(4, ...days.map((day) => day.total)),
    };
  });

  let hoverStat = $derived(
    syncStats.days.find((day) => day.date === hoverStatDate) || null,
  );

  loadConfig();

  onMount(() => {
    loadLogs();
    loadSyncMeta();
    backfillSyncMetaFromLogs();
    loadAutoStatus(); // 恢复后端监控状态
    if (!localFile) autoDetectFile(); // 未保存过路径时自动探测默认目录
    testConnections(true); // 打开页面自动检测连接（静默模式，不写提醒日志）
    startHealthCheck();
    initWindowState();
  });

  /* ---------- 自定义标题栏 ---------- */

  function isTauriRuntime() {
    return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  }

  async function syncMaximizedState() {
    if (!isTauriRuntime()) return;
    try {
      isMaximized = await getCurrentWindow().isMaximized();
    } catch {
      // 窗口状态读取失败时保持当前图标状态
    }
  }

  function initWindowState() {
    if (!isTauriRuntime()) return;
    syncMaximizedState();
    getCurrentWindow()
      .onResized(syncMaximizedState)
      .then((unlisten) => {
        unlistenWindowResize = unlisten;
      })
      .catch(() => {
        // 浏览器预览或权限缺失时无需监听
      });
  }

  async function minimizeWindow() {
    if (!isTauriRuntime()) return;
    await getCurrentWindow().minimize();
  }

  async function toggleMaximizeWindow() {
    if (!isTauriRuntime()) return;
    await getCurrentWindow().toggleMaximize();
    await syncMaximizedState();
  }

  async function closeWindow() {
    if (!isTauriRuntime()) return;
    await getCurrentWindow().close();
  }

  /* ---------- 配置持久化 ---------- */

  /* ---------- 后台服务心跳 ---------- */

  async function checkServerHealth() {
    try {
      const data = await backend.health();
      setServerOnline(Boolean(data?.success));
    } catch {
      setServerOnline(false);
    }
  }

  function setServerOnline(next) {
    if (next === serverOnline) return;
    serverOnline = next;
    addLog(
      next
        ? "后台服务已恢复连接"
        : "后台服务已断开，网页正在等待服务重新启动",
      next ? "success" : "error",
      "system",
    );
  }

  function startHealthCheck() {
    stopHealthCheck();
    checkServerHealth();
    healthTimer = setInterval(checkServerHealth, 5000);
  }

  function stopHealthCheck() {
    if (healthTimer) {
      clearInterval(healthTimer);
      healthTimer = null;
    }
  }

  function loadConfig() {
    try {
      const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) || "{}");
      remoteHost = saved.remoteHost || DEFAULTS.remoteHost;
      remoteUser = saved.remoteUser || DEFAULTS.remoteUser;
      remotePath = saved.remotePath || DEFAULTS.remotePath;
      sshPassword = saved.sshPassword || DEFAULTS.sshPassword;
      localFile = saved.localFile || "";
      intervalMs = saved.intervalMs || 2000;
    } catch {
      resetToDefaults();
    }
  }

  function saveConfig() {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        remoteHost,
        remoteUser,
        remotePath,
        sshPassword,
        localFile,
        intervalMs,
      }),
    );
  }

  function resetToDefaults() {
    remoteHost = DEFAULTS.remoteHost;
    remoteUser = DEFAULTS.remoteUser;
    remotePath = DEFAULTS.remotePath;
    sshPassword = DEFAULTS.sshPassword;
    saveConfig();
    addLog("已恢复默认配置", "info", "system");
  }

  function pad2(n) {
    return String(n).padStart(2, "0");
  }

  function formatDateTime(value) {
    if (!value) return "";
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return "";
    return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())} ${pad2(date.getHours())}:${pad2(date.getMinutes())}:${pad2(date.getSeconds())}`;
  }

  function loadSyncMeta() {
    try {
      const saved = JSON.parse(localStorage.getItem(SYNC_META_KEY) || "{}");
      lastManualSyncAt = Number(saved.lastManualSyncAt) || 0;
      lastManualSyncOk =
        typeof saved.lastManualSyncOk === "boolean" ? saved.lastManualSyncOk : null;
      lastAutoSyncAt = Number(saved.lastAutoSyncAt) || 0;
      lastAutoSyncOk =
        typeof saved.lastAutoSyncOk === "boolean" ? saved.lastAutoSyncOk : null;
    } catch {
      // 忽略损坏的本地缓存
    }
  }

  function saveSyncMeta() {
    try {
      localStorage.setItem(
        SYNC_META_KEY,
        JSON.stringify({
          lastManualSyncAt,
          lastManualSyncOk,
          lastAutoSyncAt,
          lastAutoSyncOk,
        }),
      );
    } catch {
      // 存储写满时静默跳过
    }
  }

  function recordManualSync(ok) {
    lastManualSyncAt = Date.now();
    lastManualSyncOk = ok;
    saveSyncMeta();
  }

  function rememberAutoSync(st) {
    if (!st?.lastSyncAt) return;
    const ts = new Date(st.lastSyncAt).getTime();
    if (!ts) return;
    lastAutoSyncAt = ts;
    lastAutoSyncOk = st.lastSyncOk;
    saveSyncMeta();
  }

  function backfillSyncMetaFromLogs() {
    let changed = false;
    if (!lastManualSyncAt) {
      const found = logs.find(
        (l) =>
          l.type === "sync" &&
          (l.message.includes("同步成功") || l.message.includes("同步失败")),
      );
      if (found?.ts) {
        lastManualSyncAt = found.ts;
        lastManualSyncOk = found.level === "success";
        changed = true;
      }
    }
    if (!lastAutoSyncAt) {
      const found = logs.find(
        (l) => l.type === "auto" && (l.level === "success" || l.level === "error"),
      );
      if (found?.ts) {
        lastAutoSyncAt = found.ts;
        lastAutoSyncOk = found.level === "success";
        changed = true;
      }
    }
    if (changed) saveSyncMeta();
  }

  /* ---------- 日志（localStorage 持久化） ---------- */

  function loadLogs() {
    try {
      const saved = JSON.parse(localStorage.getItem(LOG_KEY) || "[]");
      logs = Array.isArray(saved)
        ? saved
            .slice(-1000)
            .map((l) => ({ ...l, type: l.type || "system", ts: l.ts || 0 }))
            .sort((a, b) => b.ts - a.ts)
        : [];
    } catch {
      logs = [];
    }
  }

  function addLog(message, level = "info", type = "system") {
    const now = new Date();
    const entry = {
      id: `${now.getTime()}-${Math.random().toString(36).slice(2, 8)}`,
      time: now.toLocaleTimeString("zh-CN", { hour12: false }),
      ts: now.getTime(),
      message,
      level,
      type,
    };
    logs = [entry, ...logs.slice(0, 999)];
    try {
      localStorage.setItem(LOG_KEY, JSON.stringify(logs));
    } catch {
      // 存储写满时静默跳过持久化，不影响界面
    }
  }

  function clearLogs() {
    logs = [];
    try {
      localStorage.removeItem(LOG_KEY);
    } catch {
      // 忽略
    }
  }

  /* ---------- 连接测试 ---------- */

  async function fetchModels() {
    return backend.models();
  }

  /** 只取核心字段的模型列表（用于展示和对比，避免大文件） */
  async function fetchModelSummary() {
    return backend.modelSummary();
  }

  /** 同时测试本地 CC Switch 与远程 SSH 连接，更新状态栏徽章。
 *  silent=true 时（页面自动检测）跳过"未填密码"的提醒，避免打扰 */
  async function testConnections(silent = false) {
    if (testing) return;
    testing = true;
    sshState = "unknown";
    ccState = "unknown";
    if (!silent) addLog("正在测试本地与远程连接…", "info", "connection");
    try {
      const data = await fetchModels();
      ccState = "ok";
      const count = data.models ? data.models.length : 0;
      addLog(`CC Switch 在线，获取到 ${count} 个模型`, "success", "connection");
    } catch (error) {
      ccState = "fail";
      addLog(`CC Switch 离线: ${error.message}`, "error", "connection");
    }
    if (!sshPassword) {
      sshState = "fail";
      if (!silent) addLog("SSH 未测试：请先填写 SSH 密码", "warn", "connection");
    } else {
      try {
        const data = await backend.sshTest({
          host: remoteHost,
          user: remoteUser,
          password: sshPassword,
        });
        sshState = "ok";
        addLog(`SSH 连接成功（${remoteHost}）`, "success", "connection");
      } catch (error) {
        sshState = "fail";
        addLog(`SSH 连接失败: ${error.message}`, "error", "connection");
      }
    }
    testing = false;
  }

  /** 应用配置：保存并立即重新检测连接 */
  function applyConfig() {
    saveConfig();
    addLog("配置已保存，正在检测连接…", "info", "connection");
    testConnections(false);
  }

  /* ---------- 手动同步 ---------- */

  async function doSync() {
    if (syncing) return;
    if (!sshPassword) {
      addLog("请先填写 SSH 密码", "warn", "sync");
      return;
    }

    syncing = true;
    try {
      addLog("获取本地模型列表…", "info", "sync");
      const models = await fetchModels();
      addLog(
        `获取到 ${models.models ? models.models.length : 0} 个模型`,
        "success",
        "sync",
      );

      addLog("正在同步到远程服务器…", "info", "sync");
      const data = await backend.sync({
        host: remoteHost,
        user: remoteUser,
        path: remotePath,
        password: sshPassword,
        models,
      });

      if (data.success) {
        addLog("同步成功，远程文件已更新", "success", "sync");
        recordManualSync(true);
        if (remoteOpen) await viewAndCompare({ quiet: true });
      } else {
        addLog(`同步失败: ${data.error || "未知错误"}`, "error", "sync");
        recordManualSync(false);
      }
    } catch (error) {
      addLog(`同步失败: ${error.message}`, "error", "sync");
      recordManualSync(false);
    } finally {
      syncing = false;
    }
  }

  /* ---------- 自动同步 ---------- */

  /** 探测本地模型目录文件路径并填入 */
  async function autoDetectFile() {
    if (detecting) return;
    detecting = true;
    detectState = "idle";
    detectMessage = "正在探测默认路径…";
    try {
      const data = await backend.autoDetect();
      if (data.localFile) {
        if (localFile !== data.localFile) localFile = data.localFile;
        saveConfig();
        detectState = "ok";
        detectMessage = data.localFile;
        addLog(`已探测到本地模型目录文件: ${data.localFile}`, "success", "system");
      } else {
        detectState = "warn";
        detectMessage = "未找到默认路径，请手动填写";
        addLog("未探测到本地模型目录文件，请手动填写路径", "warn", "system");
      }
    } catch (error) {
      detectState = "error";
      detectMessage = `探测失败: ${error.message}`;
      addLog(`探测失败: ${error.message}`, "error", "system");
    } finally {
      detecting = false;
    }
  }

  async function fetchAutoStatus() {
    try {
      const data = await backend.autoSyncStatus();
      return data.success ? data.status : null;
    } catch {
      return null;
    }
  }

  async function loadAutoStatus(refreshFeedback = false) {
    const st = await fetchAutoStatus();
    lastStatusAt = Date.now();
    if (st) {
      autoStatus = st;
      autoOn = st.watching;
      rememberAutoSync(st);
      // 后端仍在监控时恢复轮询（窗口刷新后状态区才能持续自动更新）
      if (st.watching) startPolling();
    }
    if (refreshFeedback) {
      statusFeedback = st
        ? `状态已刷新 · ${formatDateTime(lastStatusAt)}`
        : "状态刷新失败";
    }
  }

  function startPolling() {
    stopPolling();
    pollTimer = setInterval(async () => {
      if (!autoOn) return;
      const prevSyncAt = autoStatus?.lastSyncAt;
      const st = await fetchAutoStatus();
      if (!st) return;
      if (st.lastSyncAt && st.lastSyncAt !== prevSyncAt) {
        addLog(
          `自动同步${st.lastSyncOk ? "成功" : "失败"}: ${st.lastSyncMessage}`,
          st.lastSyncOk ? "success" : "error",
          "auto",
        );
        if (st.lastSyncOk && remoteOpen) await viewAndCompare({ quiet: true });
      }
      autoStatus = st;
      lastStatusAt = Date.now();
      rememberAutoSync(st);
    }, 2000);
  }

  function stopPolling() {
    if (pollTimer) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
  }

  async function startAutoSync() {
    if (autoBusy) return;
    if (!sshPassword) {
      addLog("请先填写 SSH 密码", "warn", "auto");
      return;
    }

    autoBusy = true;
    try {
      const data = await backend.autoSyncStart({
        host: remoteHost,
        user: remoteUser,
        path: remotePath,
        password: sshPassword,
        localFile, // 可为空：后端自动使用默认目录（~/.codex/ 等）
        intervalMs: Number(intervalMs),
      });
      if (data.success) {
        // 回填后端实际使用的文件路径
        if (data.status?.localFile && data.status.localFile !== localFile) {
          localFile = data.status.localFile;
          saveConfig();
          addLog(`本地文件路径: ${data.status.localFile}`, "info", "auto");
        }
        autoOn = true;
        autoStatus = data.status;
        lastStatusAt = Date.now();
        rememberAutoSync(data.status);
        addLog(
          `自动同步已启用，监控文件变化（间隔 ${data.status.intervalMs}ms）`,
          "success",
          "auto",
        );
        startPolling();
      } else {
        addLog(`启用自动同步失败: ${data.error || "未知错误"}`, "error", "auto");
      }
    } catch (error) {
      addLog(`启用自动同步失败: ${error.message}`, "error", "auto");
    } finally {
      autoBusy = false;
    }
  }

  async function stopAutoSync() {
    autoBusy = true;
    try {
      const data = await backend.autoSyncStop();
      if (data.success) {
        autoOn = false;
        autoStatus = data.status;
        lastStatusAt = Date.now();
        rememberAutoSync(data.status);
        addLog("自动同步已停止", "info", "auto");
      }
    } catch (error) {
      addLog(`停止自动同步失败: ${error.message}`, "error", "auto");
    } finally {
      autoBusy = false;
    }
    stopPolling();
  }

  onDestroy(() => {
    unlistenWindowResize?.();
    stopPolling();
    stopHealthCheck();
  });

  /* ---------- 调试：远程对比 ---------- */

  /** 读取远程模型目录（SSH），只保留核心字段 */
  async function loadRemoteFile() {
    const data = await backend.remoteModels({
      host: remoteHost,
      user: remoteUser,
      path: remotePath,
      password: sshPassword,
    });
    remoteParsed = Array.isArray(data.models) ? data.models : [];
    remoteContent = JSON.stringify(remoteParsed, null, 2);
  }

  /** 查看远程文件并对比本地：一次拉取本地模型 + 远程文件，产出对比结果。
   *  quiet=true 时用于同步完成后静默刷新，不重复写开始日志。 */
  async function viewAndCompare({ quiet = false } = {}) {
    if (loading) return;
    if (!sshPassword) {
      if (!quiet) addLog("请先填写 SSH 密码", "warn", "compare");
      return;
    }

    remoteOpen = true;
    loading = true;
    remoteError = "";
    remoteContent = "";
    remoteParsed = null;
    compareResult = null;
    localModels = [];
    if (!quiet) addLog("开始查看远程文件并与本地对比…", "info", "compare");
    try {
      addLog("拉取本地与远程 config.toml…", "info", "compare");
      try {
        const configData = await backend.configCompare({
          host: remoteHost,
          user: remoteUser,
          password: sshPassword,
          localFile: localFile || undefined,
          remoteConfigPath,
        });
        configCompare = configData;
        addLog(
          `config.toml 已拉取：本地${configData.local?.exists ? "存在" : "缺失"} · 远程${configData.remote?.exists ? "存在" : "缺失"}`,
          configData.local?.exists && configData.remote?.exists
            ? "success"
            : "warn",
          "compare",
        );
      } catch (configError) {
        configCompare = { error: configError.message };
        addLog(`config.toml 对比失败: ${configError.message}`, "error", "compare");
      }

      addLog("获取本地模型列表…", "info", "compare");
      const localData = await fetchModelSummary();
      localModels = Array.isArray(localData.models) ? localData.models : [];
      if (localModels.length === 0) {
        throw new Error("本地未获取到模型列表");
      }

      addLog("读取远程文件…", "info", "compare");
      await loadRemoteFile();
      if (!remoteParsed && remoteContent) {
        remoteError = "文件不是有效的模型目录 JSON，无法对比";
        addLog(remoteError, "warn", "compare");
        return;
      }
      if (!remoteContent) {
        remoteError = "(文件为空)，无法对比";
        addLog(remoteError, "warn", "compare");
        return;
      }

      compareResult = buildComparison(localModels, remoteParsed);
      if (compareResult.synced) {
        addLog(
          `对比完成：已同步（${compareResult.localCount} 个模型完全一致）`,
          "success",
          "compare",
        );
      } else {
        addLog(
          `对比完成：未同步，本地 ${compareResult.localCount} 个 / 远程 ${compareResult.remoteCount} 个`,
          "warn",
          "compare",
        );
      }
    } catch (error) {
      remoteError = error.message;
      remoteParsed = null;
      addLog(`查看/对比失败: ${error.message}`, "error", "compare");
    } finally {
      loading = false;
    }
  }

  function closeRemoteView() {
    remoteOpen = false;
    remoteContent = "";
    remoteError = "";
    remoteParsed = null;
    compareResult = null;
    localModels = [];
    configCompare = null;
  }

  /** 本地模型数组 vs 远程模型数组，产出对比报告 */
  function buildComparison(local, remote) {
    const toKey = (m) => m.slug || m.display_name || JSON.stringify(m);
    const localByKey = new Map(local.map((m) => [toKey(m), m]));
    const remoteByKey = new Map(remote.map((m) => [toKey(m), m]));
    const keys = new Set([...localByKey.keys(), ...remoteByKey.keys()]);

    const localOnly = [];
    const remoteOnly = [];
    const diffs = [];
    let same = 0;

    for (const key of keys) {
      const l = localByKey.get(key);
      const r = remoteByKey.get(key);
      if (!l) {
        remoteOnly.push(key);
        continue;
      }
      if (!r) {
        localOnly.push(key);
        continue;
      }
      if (JSON.stringify(l) === JSON.stringify(r)) {
        same++;
        continue;
      }
      // 用户关心的核心差异：思考等级
      const lLevels = (l.supported_reasoning_levels || []).map((x) => x.effort).sort();
      const rLevels = (r.supported_reasoning_levels || []).map((x) => x.effort).sort();
      if (
        l.default_reasoning_level !== r.default_reasoning_level ||
        JSON.stringify(lLevels) !== JSON.stringify(rLevels)
      ) {
        diffs.push({
          key,
          type: "levels",
          detail: `本地 默认${l.default_reasoning_level ?? "-"} [${lLevels.join(", ") || "无"}] ≠ 远程 默认${r.default_reasoning_level ?? "-"} [${rLevels.join(", ") || "无"}]`,
        });
      } else {
        diffs.push({ key, type: "other", detail: "模型其他字段存在差异" });
      }
    }

    return {
      synced: localOnly.length === 0 && remoteOnly.length === 0 && diffs.length === 0,
      localCount: local.length,
      remoteCount: remote.length,
      localOnly,
      remoteOnly,
      diffs,
      same,
    };
  }

  /** 行级 diff（LCS）：返回对齐后的行序列 */
  function diffLines(aLines, bLines) {
    const n = aLines.length;
    const m = bLines.length;
    const dp = Array.from({ length: n + 1 }, () => new Int32Array(m + 1));
    for (let i = n - 1; i >= 0; i--) {
      for (let j = m - 1; j >= 0; j--) {
        dp[i][j] =
          aLines[i] === bLines[j]
            ? dp[i + 1][j + 1] + 1
            : Math.max(dp[i + 1][j], dp[i][j + 1]);
      }
    }
    const rows = [];
    let i = 0;
    let j = 0;
    while (i < n && j < m) {
      if (aLines[i] === bLines[j]) {
        rows.push({ type: "same", a: aLines[i], b: bLines[j] });
        i++;
        j++;
      } else if (dp[i + 1][j] >= dp[i][j + 1]) {
        rows.push({ type: "del", a: aLines[i], b: "" });
        i++;
      } else {
        rows.push({ type: "add", a: "", b: bLines[j] });
        j++;
      }
    }
    while (i < n) {
      rows.push({ type: "del", a: aLines[i], b: "" });
      i++;
    }
    while (j < m) {
      rows.push({ type: "add", a: "", b: bLines[j] });
      j++;
    }
    return rows;
  }
</script>

<svelte:head>
  <title>Codex 远程模型同步</title>
</svelte:head>

<div class="app-shell">
  {#if !serverOnline}
    <div class="server-offline-banner" role="alert">
      <span class="banner-icon">⚠</span>
      <div>
        <strong>应用后端未响应</strong>
        <p>请重新启动应用；后端恢复后本提示会自动消失。</p>
      </div>
    </div>
  {/if}

  <header class="topbar" data-tauri-drag-region="deep">
    <div class="topbar-inner">
      <div class="topbar-title">
        <h1>
          <span class="hourglass {autoOn ? "active" : ""}" aria-hidden="true">⌛</span>
          Codex 远程模型同步
        </h1>
        <p>本地 CC Switch → 远程 Ubuntu</p>
      </div>
      <div class="window-controls" aria-label="窗口控制">
        <button
          type="button"
          class="window-control"
          onclick={minimizeWindow}
          aria-label="最小化"
          title="最小化"
        >
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path d="M5 10h10" />
          </svg>
        </button>
        <button
          type="button"
          class="window-control"
          onclick={toggleMaximizeWindow}
          aria-label={isMaximized ? "还原" : "最大化"}
          title={isMaximized ? "还原" : "最大化"}
        >
          {#if isMaximized}
            <svg viewBox="0 0 20 20" aria-hidden="true">
              <path d="M7 7V5.5h7.5V13H13" />
              <rect x="5" y="7" width="8" height="8" rx="0.5" />
            </svg>
          {:else}
            <svg viewBox="0 0 20 20" aria-hidden="true">
              <rect x="5" y="5" width="10" height="10" rx="0.5" />
            </svg>
          {/if}
        </button>
        <button
          type="button"
          class="window-control close"
          onclick={closeWindow}
          aria-label="关闭"
          title="关闭"
        >
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path d="m6 6 8 8M14 6l-8 8" />
          </svg>
        </button>
      </div>
    </div>
  </header>

  <div class="workspace">
    <main class="content">
      <!-- ① 配置：左主列 -->
      <section class="panel">
        <div class="panel-head">
          <h2>连接配置</h2>
          <button class="btn ghost" onclick={resetToDefaults}>恢复默认</button>
        </div>

        <div class="grid">
          <label>
            <span>本地 CC Switch API</span>
            <input value="http://127.0.0.1:15721/v1/models" disabled />
          </label>
          <label>
            <span>远程主机</span>
            <input bind:value={remoteHost} oninput={saveConfig} placeholder="192.168.31.165" />
          </label>
          <label>
            <span>远程用户名</span>
            <input bind:value={remoteUser} oninput={saveConfig} placeholder="humble" />
          </label>
          <label class="wide">
            <span>远程文件路径</span>
            <input bind:value={remotePath} oninput={saveConfig} />
          </label>
          <label class="wide">
            <span>SSH 密码</span>
            <input
              type="password"
              bind:value={sshPassword}
              oninput={saveConfig}
              autocomplete="new-password"
            />
            <small>仅保存在本机应用存储（localStorage），用于自动执行同步</small>
          </label>
        </div>

        <div class="actions">
          <button class="btn primary" onclick={applyConfig} disabled={testing}>
            {testing ? "检测中…" : "确定"}
          </button>
          <p class="hint inline">修改配置后点击「确定」，连接状态自动更新。</p>
        </div>
      </section>

      <section class="panel">
        <div class="panel-head">
          <h2>同步配置</h2>
        </div>

        <div class="grid">
          <label class="wide">
            <span>本地模型目录文件</span>
            <div class="input-with-btn">
              <input
                bind:value={localFile}
                oninput={saveConfig}
                placeholder="可留空，使用默认目录"
              />
              <button class="btn" onclick={autoDetectFile} disabled={detecting}>
                {detecting ? "探测中" : "探测"}
              </button>
            </div>
            <small class="detect-result {detectState}">
              {detectMessage || "路径可留空：后端自动使用默认目录"}
            </small>
          </label>
        </div>
      </section>

      <!-- ② 文件对比：左主列 -->
    <section class="panel compare-panel {panelState}">
    <div class="panel-head">
      <h2>调试 · 对比</h2>
      <button class="btn" onclick={viewAndCompare} disabled={loading}>
        {loading ? "读取中…" : "查看并对比"}
      </button>
    </div>

    {#if !remoteOpen}
      <p class="hint">
        点击「查看并对比」获取本地与远程模型状态，核对当前配置的模型与思考等级。
      </p>
    {:else if loading}
      <p class="hint">正在获取本地与远程数据…</p>
    {:else if remoteError}
      <pre class="remote-file error">{remoteError}</pre>
    {:else if remoteParsed}
      <div class="cc-compare">
        <div class="compare-section-head">
          <h3>CC 模型对比</h3>
        </div>

        {#if compareResult}
          <div class="compare-status {compareResult.synced ? "ok" : "bad"}">
            <span>
              {#if compareResult.synced}
                已同步 ✓ 本地与远程的 {compareResult.localCount} 个模型完全一致
              {:else}
                未同步 ✗ 共 {compareResult.localOnly.length + compareResult.remoteOnly.length + compareResult.diffs.length}
                处差异：仅本地 {compareResult.localOnly.length} · 仅远程 {compareResult.remoteOnly.length} · 内容不同 {compareResult.diffs.length}
                {#if compareResult.same > 0}（另有 {compareResult.same} 个模型一致）{/if}
              {/if}
            </span>
            <button class="btn ghost" onclick={closeRemoteView}>收起</button>
          </div>
        {/if}

        <div class="compare-grid">
          <div class="compare-col">
            <div class="col-head">
              <span>远程模型（{remoteParsed.length}）</span>
              <span class="col-mark remote">远程</span>
            </div>
            <ModelList models={remoteParsed} />
          </div>
          <div class="compare-col">
            <div class="col-head">
              <span>本地模型（{localModels.length}）</span>
              <span class="col-mark local">本地</span>
            </div>
            <ModelList models={localModels} />
          </div>
        </div>

        <div class="diff-fold">
          <button
            class="diff-fold-btn"
            onclick={() => (showDiff = !showDiff)}
            aria-expanded={showDiff}
          >
            <span>源文件对比（{diffRows.length} 行）</span>
            <span class="raw-toggle">{showDiff ? "收起 ▲" : "展开 ▼"}</span>
          </button>
          {#if showDiff}
            <div class="diff-view">
              <div class="diff-head">
                <span>源文件对比</span>
                <span class="diff-legend">
                  <span class="lg del">红 = 仅远程需删除</span>
                  <span class="lg add">绿 = 仅本地需新增</span>
                </span>
              </div>
              <div class="diff-head-row">
                <div class="diff-head-cell remote">远程源文件</div>
                <div class="diff-head-cell local">本地源文件</div>
              </div>
              <div class="diff-body">
                {#each diffRows as row, i (i)}
                  <div class="diff-row">
                    <div
                      class="diff-cell {row.type === "same" ? "same" : row.type === "del" ? "del" : "empty"}"
                    >{row.a}</div>
                    <div
                      class="diff-cell {row.type === "same" ? "same" : row.type === "add" ? "add" : "empty"}"
                    >{row.b}</div>
                  </div>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      </div>
    {:else}
      <p class="hint">
        {remoteContent ? "文件不是有效的模型目录 JSON，无法对比。" : "(文件为空)"}
      </p>
    {/if}

    {#if configCompare}
      <div class="config-section">
        <div class="config-section-head">
          <h3>config.toml</h3>
          <span class="count">本地 / 远程配置文件</span>
        </div>
        {#if configCompare.error}
          <p class="hint">拉取失败: {configCompare.error}</p>
        {:else}
          <div class="config-grid">
            <div class="compare-col">
              <div class="col-head">
                <span>本地 config.toml</span>
                <span class="col-mark local">本地</span>
              </div>
              <pre class="remote-file">
                {configCompare.local?.exists
                  ? configCompare.local.content
                  : "(文件不存在)"}
              </pre>
            </div>
            <div class="compare-col">
              <div class="col-head">
                <span>远程 config.toml</span>
                <span class="col-mark remote">远程</span>
              </div>
              <pre class="remote-file">
                {configCompare.remote?.exists
                  ? configCompare.remote.content
                  : (configCompare.remote?.error || "(文件不存在)")}
              </pre>
            </div>
          </div>
          <p class="hint">
            本地路径: <code>{configCompare.local?.path}</code>
            <br />
            远程路径: <code>{configCompare.remote?.path}</code>
          </p>
        {/if}
      </div>
    {/if}
      </section>

  <!-- ③ 日志：持久化 + 统计 -->
    <section class="panel">
    <div class="panel-head">
      <h2>日志</h2>
      <div class="seg">
        <button
          class="seg-btn {statsMode === "7d" ? "active" : ""}"
          onclick={() => (statsMode = "7d")}
        >
          近 7 天
        </button>
        <button
          class="seg-btn {statsMode === "30d" ? "active" : ""}"
          onclick={() => (statsMode = "30d")}
        >
          近 30 天
        </button>
      </div>
    </div>

    <div class="stats-card">
      <div class="stats-summary">
        <span class="count">共 {syncStats.total} 次</span>
        <span class="legend-item success">
          <span class="legend-dot"></span>
          成功 {syncStats.ok}
        </span>
        <span class="legend-item danger">
          <span class="legend-dot"></span>
          失败 {syncStats.fail}
        </span>
        <span class="stats-hint {hoverStat || selectedStatDate ? "has-value" : ""}">
          {#if hoverStat}
            {hoverStat.date} · 成功 {hoverStat.ok} / 失败 {hoverStat.fail} / 共 {hoverStat.total}
          {:else if selectedStatDate}
            已筛选 {selectedStatDate}
          {:else}
            悬停查看当天详情，点击筛选当天日志
          {/if}
        </span>
        {#if selectedStatDate}
          <button type="button" class="btn ghost stats-clear" onclick={() => (selectedStatDate = "")}>
            清除筛选
          </button>
        {/if}
      </div>
      <div class="chart">
        <div class="chart-axis">
          <span>{syncStats.max}</span>
          <span>0</span>
        </div>
        <div
          class="chart-plot days-{syncStats.days.length}"
          role="group"
          aria-label="同步次数柱状图，悬停查看详情，点击筛选当天日志"
          onmouseleave={() => (hoverStatDate = "")}
        >
          {#each syncStats.days as day (day.date)}
            <button
              type="button"
              class="chart-col {hoverStatDate === day.date ? "is-hover" : ""} {selectedStatDate === day.date ? "is-active" : ""}"
              onmouseenter={() => (hoverStatDate = day.date)}
              onclick={() => (selectedStatDate = selectedStatDate === day.date ? "" : day.date)}
            >
              <div class="chart-bar">
                {#if day.fail > 0}
                  <span
                    class="bar-segment fail"
                    style={`height:${Math.max(8, Math.round((day.fail / syncStats.max) * 100))}%`}
                  ></span>
                {/if}
                {#if day.ok > 0}
                  <span
                    class="bar-segment success"
                    style={`height:${Math.max(8, Math.round((day.ok / syncStats.max) * 100))}%`}
                  ></span>
                {/if}
                {#if day.total === 0}
                  <span class="bar-empty"></span>
                {/if}
              </div>
              {#if hoverStatDate === day.date}
                <div class="chart-tip">
                  <strong>{day.date}</strong>
                  <p class="ok">成功 {day.ok}</p>
                  <p class="fail">失败 {day.fail}</p>
                  <p>合计 {day.total}</p>
                  <small>{selectedStatDate === day.date ? "点击取消筛选" : "点击筛选当天日志"}</small>
                </div>
              {/if}
              <span class="chart-label {day.showLabel ? "" : "hidden"}">{day.label}</span>
            </button>
          {/each}
        </div>
      </div>
    </div>

    <LogTable
      {logs}
      typeText={TYPE_TEXT}
      dateFilter={selectedStatDate}
      onClear={clearLogs}
      onClearDate={() => (selectedStatDate = "")}
    />
      </section>
    </main>

    <aside class="side-rail">
      <section class="panel rail">
        <div class="panel-head">
          <h2>连接状态</h2>
        </div>

        <div class="status-list">
          <div class="status-item">
            <span class="status-label">Web 后端</span>
            <span class="status-value {serverOnline ? "ok" : "fail"}">
              <span class="dot"></span>
              {serverOnline ? "在线" : "离线"}
            </span>
          </div>
          <div class="status-item">
            <span class="status-label">SSH</span>
            <span class="status-value {sshState}">
              <span class="dot"></span>
              {SSH_TEXT[sshState]}
            </span>
          </div>
          <div class="status-item">
            <span class="status-label">CC Switch</span>
            <span class="status-value {ccState}">
              <span class="dot"></span>
              {CC_TEXT[ccState]}
            </span>
          </div>
        </div>

        <button class="btn primary full-btn" onclick={testConnections} disabled={testing}>
          {testing ? "测试中…" : "测试连接"}
        </button>
      </section>

      <section class="panel rail">
        <div class="panel-head">
          <h2>同步</h2>
          <span class="auto-state {autoOn ? (autoStatus?.syncing ? "syncing" : "on") : ""}">
            <span class="dot"></span>
            {autoStatus?.syncing ? "同步中…" : autoOn ? "监控中" : "已停止"}
          </span>
        </div>

        <div class="sync-block">
          <div class="sync-block-head">
            <h3>手动同步</h3>
          </div>
          <button class="btn primary full-btn" onclick={doSync} disabled={syncing}>
            {syncing ? "同步中…" : "立即同步"}
          </button>
          <p class="hint compact last-time">
            {#if lastManualSyncAt}
              上次同步: {formatDateTime(lastManualSyncAt)}{#if lastManualSyncOk === true}（成功）{:else if lastManualSyncOk === false}（失败）{/if}
            {:else}
              尚未同步。读取 CC Switch 模型列表并上传。
            {/if}
          </p>
        </div>

        <div class="sync-divider"></div>

        <div class="sync-block">
          <div class="sync-block-head">
            <h3>自动同步</h3>
          </div>
          <div class="control-row">
            <label>
              <span>间隔</span>
              <select bind:value={intervalMs} onchange={saveConfig}>
                <option value={1000}>1s</option>
                <option value={2000}>2s</option>
                <option value={5000}>5s</option>
                <option value={10000}>10s</option>
              </select>
            </label>
            {#if !autoOn}
              <button class="btn primary" onclick={startAutoSync} disabled={autoBusy}>
                {autoBusy ? "启动中" : "启用"}
              </button>
            {:else}
              <button class="btn" onclick={stopAutoSync} disabled={autoBusy}>
                {autoBusy ? "停止中" : "停止"}
              </button>
            {/if}
          </div>
          <button
            class="btn ghost status-refresh"
            onclick={() => loadAutoStatus(true)}
          >
            刷新结果
          </button>
          {#if statusFeedback}
            <p class="hint compact status-feedback">{statusFeedback}</p>
          {/if}
          <div class="rail-auto-status">
            {#if autoStatus}
              <p>
                <span>文件</span>
                <strong>{autoStatus.fileExists ? "存在" : "不存在"}</strong>
              </p>
              <p>
                <span>同步次数</span>
                <strong>{autoStatus.totalSyncs}</strong>
              </p>
            {/if}
            <p>
              <span>最近刷新</span>
              <strong>{formatDateTime(autoRefreshAt) || "—"}</strong>
            </p>
            <p class="auto-sync-result {autoSyncOkShown === false ? "err" : autoSyncOkShown ? "ok" : ""}">
              <span>最近同步</span>
              <strong>
                {#if autoSyncAt}
                  {autoSyncOkShown ? "成功" : "失败"} · {formatDateTime(autoSyncAt)}
                {:else}
                  —
                {/if}
              </strong>
            </p>
          </div>
        </div>
      </section>
    </aside>
  </div>
</div>
