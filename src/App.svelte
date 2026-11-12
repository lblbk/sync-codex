<script>
  import { onDestroy, onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { backend } from "./api.js";
  import ModelList from "./ModelList.svelte";
  import LogTable from "./LogTable.svelte";

  const STORAGE_KEY = "codex_sync_profiles_v2";
  const LEGACY_STORAGE_KEY = "codex_sync_config";
  const LOG_KEY = "codex_sync_logs";
  const SYNC_META_KEY = "codex_sync_meta";
  const REMOTE_CATALOG_FILE = "cc-switch-model-catalog.json";

  const DEFAULTS = {
    remoteHost: "192.168.31.165",
    remoteUser: "humble",
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

  function makeProfileId() {
    return `computer-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
  }

  function createProfile(seed = {}) {
    return {
      id: seed.id || makeProfileId(),
      name: seed.name || "",
      host: seed.host || "",
      user: seed.user || "",
      password: seed.password || "",
      home: seed.home || "",
    };
  }

  function fallbackRemoteHome(user) {
    const value = String(user || "").trim();
    if (!value) return "";
    return value === "root" ? "/root" : `/home/${value}`;
  }

  function remoteCatalogPath(profile) {
    const home = String(profile?.home || fallbackRemoteHome(profile?.user)).replace(
      /\/+$/,
      "",
    );
    return home ? `${home}/.codex/${REMOTE_CATALOG_FILE}` : "";
  }

  // ---------- 电脑标签与配置 ----------
  let profiles = $state([]);
  let activeProfileId = $state("");
  let activeProfile = $derived(
    profiles.find((profile) => profile.id === activeProfileId) || null,
  );
  let activeTabId = $state("home"); // home | new | profile id
  let newProfileDraft = $state(null);
  let newProfileError = $state("");
  let profileStates = $state({});
  let remoteHost = $derived(activeProfile?.host || "");
  let remoteUser = $derived(activeProfile?.user || "");
  let remotePath = $derived(remoteCatalogPath(activeProfile));
  let sshPassword = $derived(activeProfile?.password || "");
  let sshState = $state("unknown"); // unknown | ok | fail
  let editingProfile = $state(false);
  let profileDraft = $state(null);
  let profileEditError = $state("");
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

  function normalizeProfile(raw, index) {
    return createProfile({
      id: raw?.id,
      name: raw?.name || `电脑 ${index + 1}`,
      host: raw?.host || raw?.remoteHost || "",
      user: raw?.user || raw?.remoteUser || "",
      password: raw?.password || raw?.sshPassword || "",
      home: raw?.home || "",
    });
  }

  function loadConfig() {
    try {
      const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) || "null");
      const legacy = JSON.parse(
        localStorage.getItem(LEGACY_STORAGE_KEY) || "null",
      );
      const savedProfiles = Array.isArray(saved?.profiles)
        ? saved.profiles
        : [];
      const migratedProfiles = savedProfiles.length
        ? savedProfiles
        : legacy
          ? [legacy]
          : [];

      profiles = migratedProfiles
        .map(normalizeProfile)
        .filter(
          (profile) =>
            profile.host.trim() && profile.user.trim() && profile.password,
        );
      activeProfileId = profiles.some(
        (profile) =>
          profile.id === saved?.activeProfileId &&
          profile.host.trim() &&
          profile.user.trim() &&
          profile.password,
      )
        ? saved.activeProfileId
        : profiles.find(
            (profile) =>
              profile.host.trim() && profile.user.trim() && profile.password,
          )?.id || "";
      activeTabId = "home";
      localFile = saved?.localFile || legacy?.localFile || "";
      intervalMs = saved?.intervalMs || legacy?.intervalMs || 2000;
      saveConfig();
    } catch {
      profiles = [];
      activeProfileId = "";
      activeTabId = "home";
    }
  }

  function saveConfig() {
    try {
      localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({
          version: 3,
          profiles,
          activeProfileId,
          activeTabId,
          localFile,
          intervalMs,
        }),
      );
    } catch {
      // 本地存储不可用时不阻断当前操作
    }
  }

  async function showHome() {
    if (autoOn) await stopAutoSync();
    if (editingProfile) cancelProfileEdit();
    newProfileDraft = null;
    newProfileError = "";
    activeTabId = "home";
    try {
      localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({
          version: 3,
          profiles,
          activeProfileId,
          activeTabId,
          localFile,
          intervalMs,
        }),
      );
    } catch {
      // 首页不依赖本地存储是否可写
    }
  }

  function updateProfile(id, field, value, persist = true) {
    profiles = profiles.map((profile) => {
      if (profile.id !== id) return profile;
      return {
        ...profile,
        [field]: value,
        ...(field === "user" || field === "host" ? { home: "" } : {}),
      };
    });
    if (persist && ["host", "user", "password"].includes(field)) {
      sshState = "unknown";
    }
    if (persist) saveConfig();
  }

  function profileState(id) {
    if (id === activeProfileId) return sshState;
    return profileStates[id] || "unknown";
  }

  function profileInitial(profile) {
    const name = String(profile?.name || "").trim();
    return name ? name.slice(0, 1).toUpperCase() : "?";
  }

  function beginProfileEdit() {
    profileDraft = { ...activeProfile };
    profileEditError = "";
    editingProfile = true;
  }

  function updateProfileDraft(field, value) {
    if (!profileDraft) return;
    profileDraft = {
      ...profileDraft,
      [field]: value,
      ...(field === "user" || field === "host" ? { home: "" } : {}),
    };
    profileEditError = "";
  }

  function cancelProfileEdit() {
    editingProfile = false;
    profileDraft = null;
    profileEditError = "";
  }

  async function confirmProfileEdit() {
    if (!profileDraft || !activeProfile) return;
    const host = profileDraft.host.trim();
    const user = profileDraft.user.trim();
    if (!host || !user || !profileDraft.password) {
      profileEditError = "请完整填写远程主机、用户名和 SSH 密码。";
      return;
    }
    const id = activeProfile.id;
    const index = profiles.findIndex((profile) => profile.id === id);
    profiles = profiles.map((profile) =>
      profile.id === id
        ? createProfile({
            ...profileDraft,
            id,
            host,
            user,
            name: profileDraft.name.trim() || `电脑 ${index + 1}`,
          })
        : profile,
    );
    editingProfile = false;
    profileDraft = null;
    profileEditError = "";
    sshState = "unknown";
    saveConfig();
    addLog("电脑配置已更新，正在检测连接…", "info", "connection");
    await testConnections(false);
  }

  function discardUnsavedRemoval(profile) {
    const complete =
      profile.host.trim() && profile.user.trim() && profile.password;
    if (!complete) {
      profiles = profiles.filter((item) => item.id !== profile.id);
      profileStates = Object.fromEntries(
        Object.entries(profileStates).filter(([id]) => id !== profile.id),
      );
      if (activeProfileId === profile.id) {
        activeProfileId = "";
        sshState = "unknown";
        activeTabId = "home";
      }
      saveConfig();
      return true;
    }
    return false;
  }

  async function openNewProfileTab() {
    if (autoOn) await stopAutoSync();
    if (editingProfile) cancelProfileEdit();
    newProfileDraft = createProfile({ name: `电脑 ${profiles.length + 1}` });
    newProfileError = "";
    activeTabId = "new";
    saveConfig();
  }

  function updateNewProfileDraft(field, value) {
    if (!newProfileDraft) return;
    newProfileDraft = {
      ...newProfileDraft,
      [field]: value,
      ...(field === "user" || field === "host" ? { home: "" } : {}),
    };
    newProfileError = "";
  }

  function closeNewProfileTab() {
    newProfileDraft = null;
    newProfileError = "";
    activeTabId = "home";
    saveConfig();
  }

  async function createNewProfile() {
    if (!newProfileDraft) return;
    const host = newProfileDraft.host.trim();
    const user = newProfileDraft.user.trim();
    if (!host || !user || !newProfileDraft.password) {
      newProfileError = "请完整填写远程主机、用户名和 SSH 密码。";
      return;
    }
    const profile = createProfile({
      ...newProfileDraft,
      host,
      user,
      name: newProfileDraft.name.trim() || `电脑 ${profiles.length + 1}`,
    });
    profiles = [...profiles, profile];
    activeProfileId = profile.id;
    activeTabId = profile.id;
    newProfileDraft = null;
    newProfileError = "";
    sshState = "unknown";
    saveConfig();
    addLog(`已创建电脑配置 ${profile.name}`, "info", "system");
    await testConnections(false);
  }

  function openHome() {
    if (editingProfile) cancelProfileEdit();
    activeTabId = "home";
    saveConfig();
  }

  async function activateProfile(id) {
    const profile = profiles.find((item) => item.id === id);
    if (!profile) return;
    if (autoOn) await stopAutoSync();
    editingProfile = false;
    profileDraft = null;
    activeTabId = id;
    if (activeProfileId !== id) {
      activeProfileId = id;
      sshState = "unknown";
      saveConfig();
      addLog(
        `已切换到 ${profile.name || profile.host || "电脑配置"}`,
        "info",
        "connection",
      );
    }
    await testConnections(false);
  }

  async function removeProfile(id) {
    const index = profiles.findIndex((profile) => profile.id === id);
    if (index < 0) return;
    const profile = profiles[index];
    if (discardUnsavedRemoval(profile)) return;
    if (!window.confirm(`删除「${profile.name || profile.host || "电脑配置"}」？`)) {
      return;
    }
    const deletingActive = id === activeProfileId;
    if (autoOn && deletingActive) await stopAutoSync();
    const remaining = profiles.filter((item) => item.id !== id);
    profiles = remaining;
    delete profileStates[id];
    if (deletingActive || activeTabId === id) {
      const next = remaining[Math.min(index, remaining.length - 1)] || null;
      activeProfileId = next?.id || "";
      sshState = "unknown";
      editingProfile = false;
      profileDraft = null;
      activeTabId = next?.id || "home";
    }
    saveConfig();
    addLog("已删除电脑配置", "info", "system");
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
    const testedProfileId = activeProfileId;
    const testedHost = remoteHost;
    const testedUser = remoteUser;
    const testedPassword = sshPassword;
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
    if (!testedPassword) {
      sshState = "fail";
      if (!silent) addLog("SSH 未测试：请先填写 SSH 密码", "warn", "connection");
    } else {
      try {
        const data = await backend.sshTest({
          host: testedHost,
          user: testedUser,
          password: testedPassword,
        });
        if (data.home && activeProfileId === testedProfileId) {
          updateProfile(testedProfileId, "home", String(data.home).trim());
        }
        sshState = "ok";
        profileStates[testedProfileId] = "ok";
        addLog(`SSH 连接成功（${testedHost}）`, "success", "connection");
      } catch (error) {
        sshState = "fail";
        profileStates[testedProfileId] = "fail";
        addLog(`SSH 连接失败: ${error.message}`, "error", "connection");
      }
    }
    if (sshState === "fail") profileStates[testedProfileId] = "fail";
    testing = false;
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

  <div class="browser-tabs" role="tablist" aria-label="电脑工作区">
    <button
      type="button"
      class="browser-tab home-tab"
      class:active={activeTabId === "home"}
      onclick={showHome}
      title="首页"
      aria-label="首页"
    >
      <svg viewBox="0 0 20 20" aria-hidden="true">
        <path d="M3.5 9.2 10 3.6l6.5 5.6v7.2H12v-4H8v4H3.5Z" />
      </svg>
    </button>

    {#each profiles as profile, index (profile.id)}
      {@const isActive = activeTabId === profile.id}
      <div class="browser-tab" class:active={isActive} role="tab">
        <button
          type="button"
          class="browser-tab-select"
          onclick={() => activateProfile(profile.id)}
          title={`${profile.name || `电脑 ${index + 1}`} · ${profile.host}`}
        >
          <span class="tab-favicon">{profileInitial(profile)}</span>
          <span class="browser-tab-label">{profile.name || `电脑 ${index + 1}`}</span>
          <span
            class="tab-status {profileState(profile.id)}"
            title={SSH_TEXT[profileState(profile.id)]}
          ></span>
        </button>
        <button
          type="button"
          class="browser-tab-remove"
          onclick={() => removeProfile(profile.id)}
          aria-label={`关闭 ${profile.name || `电脑 ${index + 1}`}`}
          title="关闭并删除配置"
        >×</button>
      </div>
    {/each}

    {#if activeTabId === "new"}
      <div class="browser-tab active new-tab" role="tab">
        <span class="browser-tab-select static">
          <span class="tab-favicon">+</span>
          <span class="browser-tab-label">新电脑</span>
        </span>
        <button
          type="button"
          class="browser-tab-remove"
          onclick={closeNewProfileTab}
          aria-label="关闭新电脑标签"
          title="关闭"
        >×</button>
      </div>
    {/if}

    <button
      type="button"
      class="new-tab-button"
      onclick={openNewProfileTab}
      aria-label="新建电脑"
      title="新建电脑"
    >+</button>
    <div class="tab-drag-fill" data-tauri-drag-region="deep"></div>
  </div>

  {#if activeTabId === "home"}
    <main class="home-page">
      <section class="home-intro">
        <h2>远程电脑</h2>
        <button class="btn primary" onclick={openNewProfileTab}>新建电脑</button>
      </section>

      {#if profiles.length === 0}
        <section class="home-empty">
          <div class="home-symbol" aria-hidden="true">
            <svg viewBox="0 0 64 64">
              <rect x="12" y="16" width="40" height="29" rx="3" />
              <path d="M24 51h16M32 45v6" />
            </svg>
          </div>
          <h3>还没有电脑配置</h3>
          <p>新建一台电脑并填写连接信息，之后可从这里切换。</p>
          <button class="btn primary" onclick={openNewProfileTab}>新建电脑</button>
        </section>
      {:else}
        <section class="home-grid" aria-label="电脑列表">
          {#each profiles as profile, index (profile.id)}
            <button
              type="button"
              class="home-card"
              onclick={() => activateProfile(profile.id)}
            >
              <span class="home-card-favicon">{profileInitial(profile)}</span>
              <span class="home-card-body">
                <strong>{profile.name || `电脑 ${index + 1}`}</strong>
                <small>{profile.user}@{profile.host}</small>
              </span>
              <span class="tab-status {profileState(profile.id)}"></span>
            </button>
          {/each}
        </section>
      {/if}
    </main>
  {:else if activeTabId === "new"}
    <main class="new-profile-page">
      <section class="new-profile-card">
        <div class="new-profile-heading">
          <span class="new-profile-icon">+</span>
          <div>
            <h2>新建电脑</h2>
            <p>填写 SSH 连接信息后即可开始同步。</p>
          </div>
        </div>

        <div class="new-profile-form">
          <label>
            <span>电脑名称</span>
            <input
              value={newProfileDraft?.name || ""}
              oninput={(event) => updateNewProfileDraft("name", event.currentTarget.value)}
              placeholder="例如：办公室电脑"
              autocomplete="off"
            />
          </label>
          <label>
            <span>远程主机</span>
            <input
              value={newProfileDraft?.host || ""}
              oninput={(event) => updateNewProfileDraft("host", event.currentTarget.value)}
              placeholder="192.168.31.98"
              inputmode="url"
              autocomplete="off"
            />
          </label>
          <label>
            <span>用户名</span>
            <input
              value={newProfileDraft?.user || ""}
              oninput={(event) => updateNewProfileDraft("user", event.currentTarget.value)}
              placeholder="humble"
              autocomplete="username"
            />
          </label>
          <label>
            <span>SSH 密码</span>
            <input
              type="password"
              value={newProfileDraft?.password || ""}
              oninput={(event) => updateNewProfileDraft("password", event.currentTarget.value)}
              placeholder="输入密码"
              autocomplete="current-password"
            />
          </label>
        </div>

        <div class="new-profile-path">
          <span>远程文件</span>
          <code>{remoteCatalogPath(newProfileDraft) || "填写用户名后自动生成"}</code>
        </div>

        {#if newProfileError}
          <p class="form-error" role="alert">{newProfileError}</p>
        {/if}

        <div class="new-profile-actions">
          <button class="btn ghost" onclick={closeNewProfileTab}>取消</button>
          <button class="btn primary" onclick={createNewProfile}>创建并连接</button>
        </div>
      </section>
    </main>
  {:else if activeProfile}
    <div class="workspace">
      <main class="content">
        <section class="panel profile-config-card">
          <div class="panel-head">
            <div>
              <h2>配置</h2>
              <p class="panel-subtitle">
                {activeProfile.name} · {activeProfile.user}@{activeProfile.host}
              </p>
            </div>
            <span class="connection-pill {sshState}">
              <span class="dot"></span>
              {SSH_TEXT[sshState]}
            </span>
          </div>

          {#if !editingProfile}
            <div class="profile-readonly-grid">
              <div class="profile-readonly-item">
                <span>电脑名称</span>
                <strong>{activeProfile.name || "未命名电脑"}</strong>
              </div>
              <div class="profile-readonly-item">
                <span>远程主机</span>
                <strong>{activeProfile.host || "未配置"}</strong>
              </div>
              <div class="profile-readonly-item">
                <span>用户名</span>
                <strong>{activeProfile.user || "未配置"}</strong>
              </div>
              <div class="profile-readonly-item">
                <span>SSH 密码</span>
                <strong class:empty={!activeProfile.password}>
                  {activeProfile.password ? "••••••" : "未设置"}
                </strong>
              </div>
              <div class="profile-readonly-item wide">
                <span>远程文件</span>
                <code>{remoteCatalogPath(activeProfile) || "填写用户名后自动生成"}</code>
              </div>
            </div>
            <div class="profile-actions">
              <button class="btn" onclick={beginProfileEdit}>修改</button>
              <button class="btn primary" onclick={() => testConnections(false)} disabled={testing}>
                {testing ? "测试中…" : "测试"}
              </button>
            </div>
          {:else}
            <div class="profile-fields">
              <label>
                <span>电脑名称</span>
                <input
                  value={profileDraft?.name || ""}
                  oninput={(event) => updateProfileDraft("name", event.currentTarget.value)}
                  placeholder="例如：办公室电脑"
                  autocomplete="off"
                />
              </label>
              <label>
                <span>远程主机</span>
                <input
                  value={profileDraft?.host || ""}
                  oninput={(event) => updateProfileDraft("host", event.currentTarget.value)}
                  placeholder="192.168.31.98"
                  inputmode="url"
                  autocomplete="off"
                />
              </label>
              <label>
                <span>用户名</span>
                <input
                  value={profileDraft?.user || ""}
                  oninput={(event) => updateProfileDraft("user", event.currentTarget.value)}
                  placeholder="humble"
                  autocomplete="username"
                />
              </label>
              <label>
                <span>SSH 密码</span>
                <input
                  type="password"
                  value={profileDraft?.password || ""}
                  oninput={(event) => updateProfileDraft("password", event.currentTarget.value)}
                  placeholder="输入密码"
                  autocomplete="current-password"
                />
              </label>
            </div>

            <div class="auto-path-field">
              <div class="auto-path-label">
                <span>远程文件</span>
                <span class="auto-tag">{profileDraft?.home ? "已定位" : "自动生成"}</span>
              </div>
              <code title={remoteCatalogPath(profileDraft)}>
                {remoteCatalogPath(profileDraft) || "填写用户名后自动生成"}
              </code>
            </div>

            {#if profileEditError}
              <p class="form-error" role="alert">{profileEditError}</p>
            {/if}

            <div class="profile-actions">
              <button class="btn ghost" onclick={cancelProfileEdit}>取消</button>
              <button class="btn primary" onclick={confirmProfileEdit}>确定</button>
            </div>
          {/if}

          <div class="config-sync-section">
            <div class="config-sync-head">
              <h3>同步路径</h3>
              <span>本地 CC Switch 模型目录</span>
            </div>
            <div class="config-path-field">
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
            </div>
          </div>
        </section>

      <section class="panel compare-panel {panelState}">
    <div class="panel-head">
      <h2>调试</h2>
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

    <section class="panel log-card">
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
  {/if}
</div>
