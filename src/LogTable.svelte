<script>
  let { logs = [], typeText = {}, dateFilter = "", onClear = () => {}, onClearDate = () => {} } = $props();

  const LEVEL_TEXT = {
    success: "成功",
    error: "失败",
    warn: "警告",
    info: "信息",
  };

  const PAGE_SIZES = [10, 20, 50, 100];

  let query = $state("");
  let typeFilter = $state("all");
  let levelFilter = $state("all");
  let sortKey = $state("ts");
  let sortDir = $state("desc");
  let page = $state(1);
  let pageSize = $state(20);

  let typeOptions = $derived([
    { id: "all", label: "全部类型" },
    { id: "sync", label: typeText.sync || "手动同步" },
    { id: "auto", label: typeText.auto || "自动同步" },
    { id: "connection", label: typeText.connection || "连接" },
    { id: "compare", label: typeText.compare || "对比" },
    { id: "system", label: typeText.system || "系统" },
  ]);

  let levelOptions = $derived([
    { id: "all", label: "全部级别" },
    { id: "success", label: "成功" },
    { id: "error", label: "失败" },
    { id: "warn", label: "警告" },
    { id: "info", label: "信息" },
  ]);

  function pad(n) {
    return String(n).padStart(2, "0");
  }

  function formatDateTime(ts, fallback = "") {
    if (!ts) return fallback || "—";
    const date = new Date(ts);
    if (Number.isNaN(date.getTime())) return "—";
    return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
  }

  function logKey(log, index) {
    return log.id || `${log.ts}-${log.type}-${log.message}-${index}`;
  }

  function logDateKey(ts) {
    if (!ts) return "";
    const date = new Date(ts);
    if (Number.isNaN(date.getTime())) return "";
    return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  }

  function matchesQuery(log, q) {
    if (!q) return true;
    const typeLabel = typeText[log.type] || "系统";
    const levelLabel = LEVEL_TEXT[log.level] || log.level || "";
    const hay = `${formatDateTime(log.ts)} ${log.time || ""} ${typeLabel} ${levelLabel} ${log.message || ""}`.toLowerCase();
    return hay.includes(q);
  }

  let filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const rows = logs.filter((log) => {
      if (dateFilter && logDateKey(log.ts) !== dateFilter) return false;
      if (typeFilter !== "all" && log.type !== typeFilter) return false;
      if (levelFilter !== "all" && log.level !== levelFilter) return false;
      return matchesQuery(log, q);
    });

    const dir = sortDir === "asc" ? 1 : -1;
    return rows.slice().sort((a, b) => {
      if (sortKey === "ts") {
        return ((Number(a.ts) || 0) - (Number(b.ts) || 0)) * dir;
      }
      let av = "";
      let bv = "";
      if (sortKey === "type") {
        av = typeText[a.type] || a.type || "";
        bv = typeText[b.type] || b.type || "";
      } else if (sortKey === "level") {
        av = LEVEL_TEXT[a.level] || a.level || "";
        bv = LEVEL_TEXT[b.level] || b.level || "";
      } else {
        av = a.message || "";
        bv = b.message || "";
      }
      return String(av).localeCompare(String(bv), "zh-CN") * dir;
    });
  });

  let pageLimit = $derived(Number(pageSize) || 20);
  let totalPages = $derived(Math.max(1, Math.ceil(filtered.length / pageLimit) || 1));
  let currentPage = $derived(Math.min(Math.max(1, page), totalPages));
  let paged = $derived.by(() => {
    const start = (currentPage - 1) * pageLimit;
    return filtered.slice(start, start + pageLimit);
  });
  let rangeStart = $derived(filtered.length === 0 ? 0 : (currentPage - 1) * pageLimit + 1);
  let rangeEnd = $derived(Math.min(currentPage * pageLimit, filtered.length));

  function resetPage() {
    page = 1;
  }

  $effect(() => {
    dateFilter;
    page = 1;
  });

  function toggleSort(key) {
    if (sortKey === key) {
      sortDir = sortDir === "asc" ? "desc" : "asc";
    } else {
      sortKey = key;
      sortDir = key === "ts" ? "desc" : "asc";
    }
  }

  function sortState(key) {
    if (sortKey !== key) return "none";
    return sortDir === "asc" ? "ascending" : "descending";
  }

  function sortMark(key) {
    if (sortKey !== key) return "↕";
    return sortDir === "asc" ? "↑" : "↓";
  }

  function csvCell(value) {
    const text = String(value ?? "");
    if (/[",\n]/.test(text)) return `"${text.replace(/"/g, '""')}"`;
    return text;
  }

  function exportCsv() {
    const header = ["时间", "类型", "级别", "内容"];
    const lines = [
      header.join(","),
      ...filtered.map((log) =>
        [
          csvCell(formatDateTime(log.ts)),
          csvCell(typeText[log.type] || "系统"),
          csvCell(LEVEL_TEXT[log.level] || log.level || ""),
          csvCell(log.message || ""),
        ].join(","),
      ),
    ];
    const blob = new Blob(["\uFEFF" + lines.join("\n")], {
      type: "text/csv;charset=utf-8",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    const stamp = formatDateTime(Date.now()).replace(/[: ]/g, "-");
    a.href = url;
    a.download = `codex-sync-logs-${stamp}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  }

  function handleClear() {
    if (!logs.length) return;
    if (!confirm(`确定清空全部 ${logs.length} 条日志？此操作不可恢复。`)) return;
    onClear();
    query = "";
    typeFilter = "all";
    levelFilter = "all";
    page = 1;
  }
</script>

<div class="log-table">
  {#if dateFilter}
    <div class="log-date-filter">
      正在查看 <strong>{dateFilter}</strong> 的日志
      <button type="button" class="btn ghost" onclick={onClearDate}>清除日期筛选</button>
    </div>
  {/if}

  <div class="log-toolbar">
    <label class="log-search">
      <span class="sr-only">搜索日志</span>
      <input
        type="search"
        placeholder="搜索时间、类型或内容"
        bind:value={query}
        oninput={resetPage}
      />
    </label>
    <select bind:value={typeFilter} onchange={resetPage} aria-label="按类型筛选">
      {#each typeOptions as option}
        <option value={option.id}>{option.label}</option>
      {/each}
    </select>
    <select bind:value={levelFilter} onchange={resetPage} aria-label="按级别筛选">
      {#each levelOptions as option}
        <option value={option.id}>{option.label}</option>
      {/each}
    </select>
    <select
      bind:value={pageSize}
      onchange={resetPage}
      aria-label="每页条数"
    >
      {#each PAGE_SIZES as size}
        <option value={size}>{size} 条/页</option>
      {/each}
    </select>
  </div>

  <div class="log-frame" role="log">
    {#if logs.length === 0}
      <p class="log-empty">暂无日志</p>
    {:else if filtered.length === 0}
      <p class="log-empty">没有符合筛选条件的日志</p>
    {:else}
      <table>
        <thead>
          <tr>
            <th aria-sort={sortState("ts")}>
              <button type="button" onclick={() => toggleSort("ts")}>
                时间 <span class="sort">{sortMark("ts")}</span>
              </button>
            </th>
            <th aria-sort={sortState("type")}>
              <button type="button" onclick={() => toggleSort("type")}>
                类型 <span class="sort">{sortMark("type")}</span>
              </button>
            </th>
            <th aria-sort={sortState("level")}>
              <button type="button" onclick={() => toggleSort("level")}>
                级别 <span class="sort">{sortMark("level")}</span>
              </button>
            </th>
            <th aria-sort={sortState("message")}>
              <button type="button" onclick={() => toggleSort("message")}>
                内容 <span class="sort">{sortMark("message")}</span>
              </button>
            </th>
          </tr>
        </thead>
        <tbody>
          {#each paged as log, index (logKey(log, index))}
            <tr class="row-{log.level || "info"}">
              <td class="time">{formatDateTime(log.ts, log.time)}</td>
              <td>
                <span class="log-type {log.type}">{typeText[log.type] || "系统"}</span>
              </td>
              <td>
                <span class="log-level {log.level}">{LEVEL_TEXT[log.level] || log.level || "信息"}</span>
              </td>
              <td class="message {log.level}" title={log.message}>{log.message}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>

  <div class="log-footer">
    <span class="log-count">
      {#if filtered.length === 0}
        共 {logs.length} 条
      {:else}
        第 {rangeStart}-{rangeEnd} 条，筛选 {filtered.length} / 全部 {logs.length}
      {/if}
    </span>
    <div class="pager">
      <button type="button" class="btn ghost" disabled={currentPage <= 1} onclick={() => (page = currentPage - 1)}>
        上一页
      </button>
      <span class="page-num">{currentPage} / {totalPages}</span>
      <button
        type="button"
        class="btn ghost"
        disabled={currentPage >= totalPages}
        onclick={() => (page = currentPage + 1)}
      >
        下一页
      </button>
    </div>
    <div class="log-actions">
      <button type="button" class="btn ghost" onclick={exportCsv} disabled={filtered.length === 0}>导出 CSV</button>
      <button type="button" class="btn ghost" onclick={handleClear} disabled={logs.length === 0}>清空日志</button>
    </div>
  </div>
</div>

<style>
  .log-table {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .log-toolbar {
    display: grid;
    grid-template-columns: minmax(180px, 1.4fr) repeat(3, minmax(110px, 0.7fr));
    gap: 8px;
  }

  .log-search input,
  .log-toolbar select {
    width: 100%;
    height: 36px;
    padding: 0 10px;
    font: inherit;
    color: var(--text);
    background: #fff;
    border: 1px solid var(--border);
    border-radius: 8px;
    outline: none;
  }

  .log-search input:focus,
  .log-toolbar select:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px rgba(37, 99, 235, 0.14);
  }

  .log-frame {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: #fff;
    overflow: auto;
    max-height: min(480px, 52vh);
  }

  .log-empty {
    padding: 36px 16px;
    text-align: center;
    color: var(--muted);
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
  }

  th,
  td {
    padding: 8px 12px;
    text-align: left;
    vertical-align: top;
    border-bottom: 1px solid #eef1f5;
  }

  th {
    position: sticky;
    top: 0;
    z-index: 1;
    background: #f8fafc;
    font-size: 12px;
    font-weight: 650;
    color: var(--muted);
    white-space: nowrap;
  }

  th button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    font-weight: 650;
    cursor: pointer;
  }

  th .sort {
    font-size: 11px;
    color: #9aa4b2;
  }

  th button:hover {
    color: var(--text);
  }

  th button:hover .sort {
    color: var(--accent);
  }

  tbody tr {
    transition: background 0.12s ease;
  }

  tbody tr:nth-child(even) {
    background: #f7f9fc;
  }

  tbody tr:hover {
    background: #e8f1ff;
  }

  tbody tr:hover td:first-child {
    box-shadow: inset 3px 0 0 var(--accent);
  }

  tbody tr.row-error {
    background: rgba(220, 38, 38, 0.045);
  }

  tbody tr.row-error:hover {
    background: #fde8e8;
  }

  tbody tr:last-child td {
    border-bottom: none;
  }

  .log-date-filter {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 8px 10px;
    border: 1px solid rgba(37, 99, 235, 0.2);
    border-radius: 8px;
    background: rgba(37, 99, 235, 0.06);
    font-size: 12.5px;
    color: var(--text);
  }

  .time {
    width: 1%;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  .message {
    word-break: break-word;
    color: var(--text);
  }

  .message.success {
    color: var(--success);
  }

  .message.error {
    color: var(--danger);
  }

  .message.warn {
    color: var(--warn);
  }

  .log-level {
    display: inline-flex;
    align-items: center;
    padding: 0 7px;
    border-radius: 999px;
    font-size: 11px;
    line-height: 1.7;
    font-weight: 650;
    background: #eef1f5;
    color: var(--muted);
    white-space: nowrap;
  }

  .log-level.success {
    background: rgba(22, 163, 74, 0.1);
    color: var(--success);
  }

  .log-level.error {
    background: rgba(220, 38, 38, 0.1);
    color: var(--danger);
  }

  .log-level.warn {
    background: rgba(217, 119, 6, 0.1);
    color: var(--warn);
  }

  .log-level.info {
    background: rgba(37, 99, 235, 0.08);
    color: var(--accent);
  }

  .log-footer {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px 12px;
  }

  .log-count,
  .page-num {
    font-size: 12.5px;
    color: var(--muted);
  }

  .pager,
  .log-actions {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    border: 0;
  }

  @media (max-width: 720px) {
    .log-toolbar {
      grid-template-columns: 1fr 1fr;
    }

    .log-search {
      grid-column: 1 / -1;
    }
  }
</style>
