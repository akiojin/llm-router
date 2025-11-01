const REFRESH_INTERVAL_MS = 5000;
const PERFORMANCE_THRESHOLDS = Object.freeze({
  fetch: 2000,
  render: 100,
  backend: 100,
});
const AGENT_METRICS_LIMIT = 120;

const state = {
  agents: [],
  stats: null,
  history: [],
  filterStatus: "all",
  filterQuery: "",
  sortKey: "machine",
  sortOrder: "asc",
  lastFocused: null,
  selection: new Set(),
  selectAll: false,
  currentAgentId: null,
  currentPage: 1,
  pageSize: 50,
  timerId: null,
  rowCache: new Map(),
  renderSnapshot: null,
  statsSignature: "",
  historySignature: "",
  selectAllCheckbox: null,
  performanceIndicator: null,
  metrics: {
    fetchMs: null,
    renderMs: null,
    usedLegacy: false,
    backendMs: null,
    generatedAt: null,
    severity: "idle",
  },
  agentMetricsSignature: "",
  agentMetricsCache: new Map(),
  agentMetricsAbortController: null,
  fallbackNotified: false,
};

let requestsChart = null;
let agentMetricsChart = null;
const modalRefs = {
  modal: null,
  close: null,
  ok: null,
  save: null,
  delete: null,
  machineName: null,
  ipAddress: null,
  ollamaVersion: null,
  loadedModels: null,
  uptime: null,
  status: null,
  lastSeen: null,
  totalRequests: null,
  averageResponse: null,
  customName: null,
  tags: null,
  notes: null,
  gpuUsage: null,
  gpuMemory: null,
  gpuCapabilityScore: null,
  gpuModel: null,
  gpuCompute: null,
  metricsStatus: null,
  metricsCanvas: null,
};
const paginationRefs = { prev: null, next: null, info: null };

document.addEventListener("DOMContentLoaded", () => {
  const refreshButton = document.getElementById("refresh-button");
  const statusSelect = document.getElementById("filter-status");
  const queryInput = document.getElementById("filter-query");
  const sortableHeaders = document.querySelectorAll("th[data-sort]");
  const selectAllCheckbox = document.getElementById("select-all");
  const exportJsonButton = document.getElementById("export-json");
  const exportCsvButton = document.getElementById("export-csv");
  const modal = document.getElementById("agent-modal");
  const modalClose = document.getElementById("agent-modal-close");
  const modalOk = document.getElementById("agent-modal-ok");
  const modalSave = document.getElementById("agent-modal-save");
  const modalDelete = document.getElementById("agent-modal-delete");
  const modalDisconnect = document.getElementById("agent-modal-disconnect");
  const tbody = document.getElementById("agents-body");

  paginationRefs.prev = document.getElementById("page-prev");
  paginationRefs.next = document.getElementById("page-next");
  paginationRefs.info = document.getElementById("page-info");
  state.selectAllCheckbox = selectAllCheckbox;
  state.performanceIndicator = document.getElementById("refresh-metrics");
  updatePerformanceIndicator();

  Object.assign(modalRefs, {
    modal,
    close: modalClose,
    ok: modalOk,
    machineName: document.getElementById("detail-machine-name"),
    ipAddress: document.getElementById("detail-ip-address"),
    ollamaVersion: document.getElementById("detail-ollama-version"),
    loadedModels: document.getElementById("detail-loaded-models"),
    uptime: document.getElementById("detail-uptime"),
    status: document.getElementById("detail-status"),
    lastSeen: document.getElementById("detail-last-seen"),
    totalRequests: document.getElementById("detail-total-requests"),
    averageResponse: document.getElementById("detail-average-response"),
    customName: document.getElementById("detail-custom-name"),
    tags: document.getElementById("detail-tags"),
    notes: document.getElementById("detail-notes"),
    gpuUsage: document.getElementById("detail-gpu-usage"),
    gpuMemory: document.getElementById("detail-gpu-memory"),
    gpuCapabilityScore: document.getElementById("detail-gpu-capability-score"),
    gpuModel: document.getElementById("detail-gpu-model"),
    gpuCompute: document.getElementById("detail-gpu-compute"),
    save: modalSave,
    delete: modalDelete,
    disconnect: modalDisconnect,
    metricsStatus: document.getElementById("agent-metrics-status"),
    metricsCanvas: document.getElementById("agent-metrics-chart"),
  });

  refreshButton.addEventListener("click", () => refreshData({ manual: true }));
  statusSelect.addEventListener("change", (event) => {
    state.filterStatus = event.target.value;
    state.currentPage = 1;
    renderAgents();
  });
  let queryDebounce = null;
  queryInput.addEventListener("input", (event) => {
    const value = event.target.value ?? "";
    window.clearTimeout(queryDebounce);
    queryDebounce = window.setTimeout(() => {
      state.filterQuery = value.trim().toLowerCase();
      state.currentPage = 1;
      renderAgents();
    }, 150);
  });
  selectAllCheckbox.addEventListener("change", (event) => {
    state.selectAll = event.target.checked;
    if (state.selectAll) {
      const filtered = state.agents.filter((agent) =>
        filterAgent(agent, state.filterStatus, state.filterQuery),
      );
      state.selection = new Set(filtered.map((agent) => agent.id));
    } else {
      state.selection.clear();
    }
    renderAgents();
  });

  sortableHeaders.forEach((header) => {
    header.addEventListener("click", () => {
      const key = header.dataset.sort;
      if (!key) return;
      if (state.sortKey === key) {
        state.sortOrder = state.sortOrder === "asc" ? "desc" : "asc";
      } else {
        state.sortKey = key;
        state.sortOrder = "asc";
      }
      updateSortIndicators();
      renderAgents();
    });
  });

  updateSortIndicators();

  paginationRefs.prev?.addEventListener("click", () => {
    if (state.currentPage > 1) {
      state.currentPage -= 1;
      renderAgents();
    }
  });

  paginationRefs.next?.addEventListener("click", () => {
    const totalPages = calculateTotalPages();
    if (state.currentPage < totalPages) {
      state.currentPage += 1;
      renderAgents();
    }
  });

  exportJsonButton.addEventListener("click", () => {
    const data = getFilteredAgents();
    downloadJson(data, "agents.json");
  });

  exportCsvButton.addEventListener("click", () => {
    const data = getFilteredAgents();
    downloadCsv(data, "agents.csv");
  });

  tbody.addEventListener("click", (event) => {
    const rowCheckbox = event.target.closest("input[data-agent-id]");
    if (rowCheckbox) {
      const agentId = rowCheckbox.dataset.agentId;
      if (rowCheckbox.checked) {
        state.selection.add(agentId);
      } else {
        state.selection.delete(agentId);
        state.selectAll = false;
        selectAllCheckbox.checked = false;
      }
      return;
    }
    const button = event.target.closest("button[data-agent-id]");
    if (!button) return;
    const agentId = button.dataset.agentId;
    const agent = state.agents.find((item) => item.id === agentId);
    if (agent) {
      openAgentModal(agent);
    }
  });

  const closeModal = () => closeAgentModal();
  modalClose.addEventListener("click", closeModal);
  modalOk.addEventListener("click", closeModal);
  modalSave.addEventListener("click", async () => {
    if (!state.currentAgentId) return;
    const agentId = state.currentAgentId;
    try {
      const updated = await saveAgentSettings(agentId);
      if (updated && updated.id) {
        state.agents = state.agents.map((agent) =>
          agent.id === updated.id ? { ...agent, ...updated } : agent,
        );
        closeAgentModal();
        renderAgents();
      }
    } catch (error) {
      console.error("Failed to persist agent settings", error);
    }
  });
  modalDelete.addEventListener("click", async () => {
    if (!state.currentAgentId) return;
    const agentId = state.currentAgentId;
    const agent = state.agents.find((item) => item.id === agentId);
    const name = agent ? getDisplayName(agent) : "対象";
    if (!window.confirm(`${name} を削除しますか？`)) {
      return;
    }

    try {
      await deleteAgent(agentId);
      state.agents = state.agents.filter((item) => item.id !== agentId);
      state.selection.delete(agentId);
      closeAgentModal();
      renderAgents();
    } catch (error) {
      console.error("Failed to delete agent", error);
    }
  });
  modalDisconnect.addEventListener("click", async () => {
    if (!state.currentAgentId) return;
    const agentId = state.currentAgentId;
    try {
      await disconnectAgent(agentId);
      const agent = state.agents.find((item) => item.id === agentId);
      if (agent) {
        agent.status = "offline";
      }
      renderAgents();
    } catch (error) {
      console.error("Failed to disconnect agent", error);
    }
  });
  modal.addEventListener("click", (event) => {
    if (event.target === modal) {
      closeModal();
    }
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && !modal.classList.contains("hidden")) {
      closeModal();
    }
  });

  refreshData({ initial: true });
  state.timerId = window.setInterval(refreshData, REFRESH_INTERVAL_MS);

  document.addEventListener("visibilitychange", () => {
    if (document.hidden) {
      window.clearInterval(state.timerId);
      state.timerId = null;
    } else if (!state.timerId) {
      refreshData();
      state.timerId = window.setInterval(refreshData, REFRESH_INTERVAL_MS);
    }
  });
});

async function refreshData({ manual = false } = {}) {
  setConnectionStatus(manual ? "loading" : "updating");

  const start = performance.now();
  let overview;
  let fetchMs;
  let usedLegacy = false;

  try {
    try {
      overview = await fetchOverview();
      fetchMs = performance.now() - start;
      state.fallbackNotified = false;
    } catch (error) {
      if (error?.status === 404) {
        usedLegacy = true;
        const legacyStart = performance.now();
        overview = await fetchLegacyOverview();
        fetchMs = performance.now() - start;
        if (!state.fallbackNotified) {
          console.warn(
            "Dashboard overview endpoint not available; falling back to legacy endpoints.",
          );
          state.fallbackNotified = true;
        }
      } else {
        throw error;
      }
    }

    const renderStart = performance.now();
    applyOverviewData(overview);
    const renderMs = performance.now() - renderStart;
    const serverMs =
      typeof overview?.generation_time_ms === "number"
        ? overview.generation_time_ms
        : null;
    recordPerformanceMetrics(fetchMs, renderMs, { usedLegacy, serverMs });
  } catch (error) {
    handleRefreshFailure(error);
    recordPerformanceMetrics(null, null, { usedLegacy: false, serverMs: null });
  }
}

function applyOverviewData(overview) {
  state.agents = Array.isArray(overview.agents) ? overview.agents : [];
  state.stats = overview.stats ?? null;
  state.history = Array.isArray(overview.history) ? overview.history : [];
  const generatedAt =
    typeof overview.generated_at === "string" ? new Date(overview.generated_at) : null;
  state.metrics.generatedAt = generatedAt;

  renderStats();
  renderAgents();
  renderHistory();
  hideError();
  setConnectionStatus("online");
  updateLastRefreshed(new Date(), generatedAt);
}

async function fetchLegacyOverview() {
  const [agents, stats, history] = await Promise.all([
    fetchJson("/api/dashboard/agents"),
    fetchJson("/api/dashboard/stats"),
    fetchJson("/api/dashboard/request-history"),
  ]);

  return { agents, stats, history };
}

async function fetchOverview() {
  return fetchJson("/api/dashboard/overview");
}

function handleRefreshFailure(error) {
  console.error("Dashboard refresh failed:", error);
  showError(`ダッシュボードデータの取得に失敗しました: ${error?.message ?? error}`);
  setConnectionStatus("offline");
}

function recordPerformanceMetrics(fetchMs, renderMs, { usedLegacy = false, serverMs = null } = {}) {
  state.metrics.fetchMs = typeof fetchMs === "number" ? Math.round(fetchMs) : null;
  state.metrics.renderMs = typeof renderMs === "number" ? Math.round(renderMs) : null;
  state.metrics.backendMs = typeof serverMs === "number" ? Math.round(serverMs) : null;
  state.metrics.usedLegacy = Boolean(usedLegacy);
  state.metrics.severity = evaluatePerformanceSeverity();
  updatePerformanceIndicator();
}

function updatePerformanceIndicator() {
  const target = state.performanceIndicator;
  if (!target) return;

  const { fetchMs, renderMs, backendMs, usedLegacy, severity } = state.metrics;
  const segments = [
    `取得: ${fetchMs == null ? "-" : `${fetchMs} ms`}`,
    `描画: ${renderMs == null ? "-" : `${renderMs} ms`}`,
    `サーバー集計: ${backendMs == null ? "-" : `${backendMs} ms`}`,
  ];
  const suffix = usedLegacy ? " (legacy)" : "";

  target.textContent = segments.join(" / ") + suffix;
  target.classList.toggle("is-legacy", usedLegacy);
  target.classList.toggle("is-warning", severity === "warn");
  target.classList.toggle("is-error", severity === "error");
}

function evaluatePerformanceSeverity() {
  const { fetchMs, renderMs, backendMs, usedLegacy } = state.metrics;
  const ratios = [];

  if (typeof fetchMs === "number" && PERFORMANCE_THRESHOLDS.fetch > 0) {
    ratios.push(fetchMs / PERFORMANCE_THRESHOLDS.fetch);
  }
  if (typeof renderMs === "number" && PERFORMANCE_THRESHOLDS.render > 0) {
    ratios.push(renderMs / PERFORMANCE_THRESHOLDS.render);
  }
  if (typeof backendMs === "number" && PERFORMANCE_THRESHOLDS.backend > 0) {
    ratios.push(backendMs / PERFORMANCE_THRESHOLDS.backend);
  }
  if (usedLegacy) {
    ratios.push(1.1);
  }

  if (!ratios.length) {
    return "idle";
  }

  const maxRatio = Math.max(...ratios);
  if (maxRatio >= 2) {
    return "error";
  }
  if (maxRatio > 1) {
    return "warn";
  }
  return "ok";
}

async function fetchJson(url, options = {}) {
  const { headers, ...rest } = options;
  const response = await fetch(url, {
    method: "GET",
    cache: "no-store",
    headers: {
      Accept: "application/json",
      ...(headers ?? {}),
    },
    ...rest,
  });

  if (!response.ok) {
    const error = new Error(`${response.status} ${response.statusText}`);
    error.status = response.status;
    throw error;
  }

  return response.json();
}

function renderStats() {
  if (!state.stats) {
    return;
  }

  const statsMap = {
    "total-agents": state.stats.total_agents,
    "online-agents": state.stats.online_agents,
    "offline-agents": state.stats.offline_agents,
    "total-requests": state.stats.total_requests,
    "successful-requests": state.stats.successful_requests,
    "failed-requests": state.stats.failed_requests,
    "total-active-requests": state.stats.total_active_requests,
    "average-response-time-ms": formatAverage(state.stats.average_response_time_ms),
    "average-gpu-usage": formatPercentage(state.stats.average_gpu_usage),
    "average-gpu-memory-usage": formatPercentage(state.stats.average_gpu_memory_usage),
    "last-metrics-updated-at": formatTimestamp(state.stats.last_metrics_updated_at),
    "last-registered-at": formatTimestamp(state.stats.last_registered_at),
    "last-seen-at": formatTimestamp(state.stats.last_seen_at),
  };

  const entries = Object.entries(statsMap);
  const nextSignature = entries
    .map(([key, value]) => `${key}:${String(value ?? "-")}`)
    .join("|");
  if (state.statsSignature === nextSignature) {
    return;
  }
  state.statsSignature = nextSignature;

  entries.forEach(([key, value]) => {
    const target = document.querySelector(`[data-stat="${key}"]`);
    if (target) {
      const displayValue = value ?? "-";
      if (target.textContent !== String(displayValue)) {
        target.textContent = displayValue;
      }
    }
  });
}

function renderAgents() {
  const tbody = document.getElementById("agents-body");
  if (!tbody) return;

  if (!state.agents.length) {
    state.rowCache.clear();
    state.renderSnapshot = null;
    state.selectAll = false;
    if (state.selectAllCheckbox) {
      state.selectAllCheckbox.checked = false;
    }
    tbody.replaceChildren(buildPlaceholderRow("エージェントはまだ登録されていません"));
    updatePagination(0);
    return;
  }

  const filtered = getFilteredAgents();

  if (!filtered.length) {
    state.renderSnapshot = null;
    state.selectAll = false;
    if (state.selectAllCheckbox) {
      state.selectAllCheckbox.checked = false;
    }
    tbody.replaceChildren(buildPlaceholderRow("条件に一致するエージェントはありません"));
    updatePagination(0);
    return;
  }

  state.selectAll = filtered.every((agent) => state.selection.has(agent.id));
  if (state.selectAllCheckbox) {
    state.selectAllCheckbox.checked = state.selectAll;
  }

  const sorted = sortAgents(filtered, state.sortKey, state.sortOrder);
  const totalPages = calculateTotalPages(sorted.length);
  state.currentPage = Math.min(Math.max(state.currentPage, 1), totalPages);
  const pageSlice = paginate(sorted, state.currentPage, state.pageSize);

  const pageHash = buildPageSignature(pageSlice);
  const selectionHash = buildSelectionSignature();
  const snapshotKey = [
    pageHash,
    selectionHash,
    state.sortKey,
    state.sortOrder,
    state.currentPage,
    state.pageSize,
    state.filterStatus,
    state.filterQuery,
  ].join("#");

  if (
    state.renderSnapshot &&
    state.renderSnapshot.key === snapshotKey &&
    state.renderSnapshot.totalPages === totalPages
  ) {
    updatePagination(totalPages);
    return;
  }

  const fragment = document.createDocumentFragment();

  pageSlice.forEach((agent) => {
    const signature = getAgentSignature(agent);
    const cached = state.rowCache.get(agent.id);
    let row = cached?.node;
    if (!row) {
      row = document.createElement("tr");
    }

    if (!cached || cached.signature !== signature) {
      buildAgentRow(agent, row);
      state.rowCache.set(agent.id, { node: row, signature });
    } else {
      syncAgentRowSelection(row, agent.id);
      row.classList.toggle("agent-offline", agent.status === "offline");
    }

    fragment.appendChild(row);
  });

  tbody.replaceChildren(fragment);

  if (state.rowCache.size > state.agents.length) {
    const knownIds = new Set(state.agents.map((agent) => agent.id));
    for (const id of state.rowCache.keys()) {
      if (!knownIds.has(id)) {
        state.rowCache.delete(id);
      }
    }
  }
  updatePagination(totalPages);

  state.renderSnapshot = { key: snapshotKey, totalPages };
}

function renderHistory() {
  const canvas = document.getElementById("requests-chart");
  if (!canvas || typeof Chart === "undefined") {
    return;
  }

  const historyArray = Array.isArray(state.history) ? state.history : [];
  const nextSignature = JSON.stringify(historyArray);
  if (state.historySignature === nextSignature && requestsChart) {
    return;
  }
  state.historySignature = nextSignature;

  if (!historyArray.length) {
    const labels = buildHistoryLabels([]);
    const zeroes = new Array(labels.length).fill(0);
    updateHistoryChart(canvas, labels, zeroes, zeroes);
    return;
  }

  const labels = buildHistoryLabels(historyArray);
  const success = historyArray.map((point) => point.success ?? 0);
  const failures = historyArray.map((point) => point.error ?? 0);
  updateHistoryChart(canvas, labels, success, failures);
}

function updateHistoryChart(canvas, labels, success, failures) {
  if (!requestsChart) {
    requestsChart = new Chart(canvas, {
      type: "line",
      data: {
        labels,
        datasets: [
          {
            label: "成功リクエスト",
            data: success,
            tension: 0.3,
            borderColor: "rgba(59, 130, 246, 0.9)",
            backgroundColor: "rgba(59, 130, 246, 0.15)",
            fill: true,
            pointRadius: 0,
          },
          {
            label: "失敗リクエスト",
            data: failures,
            tension: 0.3,
            borderColor: "rgba(248, 113, 113, 0.9)",
            backgroundColor: "rgba(248, 113, 113, 0.15)",
            fill: true,
            pointRadius: 0,
          },
        ],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        interaction: {
          mode: "index",
          intersect: false,
        },
        plugins: {
          legend: {
            labels: {
              color: "#e2e8f0",
            },
          },
          tooltip: {
            callbacks: {
              title(items) {
                const raw = items[0]?.label ?? "";
                return raw;
              },
            },
          },
        },
        scales: {
          x: {
            ticks: {
              color: "#e2e8f0",
              maxRotation: 0,
            },
            grid: {
              color: "rgba(148, 163, 184, 0.08)",
            },
          },
          y: {
            beginAtZero: true,
            ticks: {
              color: "#e2e8f0",
              precision: 0,
            },
            grid: {
              color: "rgba(148, 163, 184, 0.08)",
            },
          },
        },
      },
    });
  } else {
    requestsChart.data.labels = labels;
    requestsChart.data.datasets[0].data = success;
    requestsChart.data.datasets[1].data = failures;
    requestsChart.update("none");
  }
}

function buildHistoryLabels(history) {
  if (!history.length) {
    const now = alignDateToMinute(new Date());
    return Array.from({ length: 60 }, (_, idx) => {
      const date = new Date(now.getTime() - (59 - idx) * 60 * 1000);
      return formatHistoryLabel(date);
    });
  }

  return history.map((point) => formatHistoryLabel(new Date(point.minute)));
}

function summarizeGpu(agent) {
  const devices = Array.isArray(agent.gpu_devices) ? agent.gpu_devices : [];
  const totalFromDevices = devices.reduce(
    (sum, device) => sum + (Number(device?.count) || 0),
    0
  );
  const fallbackCount = typeof agent.gpu_count === "number" ? agent.gpu_count : 0;
  const totalCount = totalFromDevices || fallbackCount;
  const primaryModel = devices.length > 0 && devices[0]?.model
    ? devices[0].model
    : agent.gpu_model;

  return {
    devices,
    totalCount,
    primaryModel,
  };
}

function buildAgentRow(agent, row = document.createElement("tr")) {
  row.dataset.agentId = agent.id;
  row.classList.toggle("agent-offline", agent.status === "offline");

  const displayName = getDisplayName(agent);
  const secondaryName = agent.custom_name
    ? agent.machine_name
    : agent.ollama_version || agent.machine_name;

  const statusLabel =
    agent.status === "online"
      ? '<span class="badge badge--online">Online</span>'
      : '<span class="badge badge--offline">Offline</span>';

  const metricsBadge = agent.metrics_stale
    ? '<span class="badge badge--stale">STALE</span>'
    : "";
  const metricsTimestamp = formatTimestamp(agent.metrics_last_updated_at);
  const metricsDetail = metricsBadge ? `${metricsBadge} ${metricsTimestamp}` : metricsTimestamp;

  const cpuDisplay = formatPercentage(agent.cpu_usage);
  const gpuSummary = summarizeGpu(agent);
  const gpuModelDisplay = gpuSummary.primaryModel
    ? `${escapeHtml(gpuSummary.primaryModel)}${
        gpuSummary.totalCount > 1 ? ` (${gpuSummary.totalCount}枚)` : ''
      }`
    : 'GPU情報取得中';
  // GPU性能スコア表示
  const gpuScoreText =
    typeof agent.gpu_capability_score === "number"
      ? ` / スコア ${agent.gpu_capability_score}`
      : "";
  const cpuGpuSub =
    typeof agent.gpu_usage === "number"
      ? `<div class="cell-sub">GPU ${formatPercentage(agent.gpu_usage)} (${gpuModelDisplay})${gpuScoreText}</div>`
      : `<div class="cell-sub">${gpuModelDisplay}${gpuScoreText}</div>`;
  const memoryDisplay = formatPercentage(agent.memory_usage);
  const memoryGpuSub =
    typeof agent.gpu_memory_usage === "number"
      ? `<div class="cell-sub">GPU ${formatPercentage(agent.gpu_memory_usage)} (${gpuModelDisplay})</div>`
      : `<div class="cell-sub">${gpuModelDisplay}</div>`;
  const models = getModelList(agent);
  const primaryModelDisplay = models.length ? models[0] : "-";
  const extraModels = models.slice(1, 4).join(", ");
  const remainderCount = Math.max(0, models.length - 4);
  const modelSub = extraModels
    ? `<div class="cell-sub">${escapeHtml(extraModels)}${
        remainderCount > 0 ? ` 他${remainderCount}件` : ""
      }</div>`
    : "";

  row.innerHTML = `
    <td>
      <input
        type="checkbox"
        data-agent-id="${agent.id}"
        ${state.selection.has(agent.id) ? "checked" : ""}
        aria-label="${escapeHtml(agent.machine_name)} を選択"
      />
    </td>
    <td>
      <div class="cell-title">${escapeHtml(displayName)}</div>
      <div class="cell-sub">${escapeHtml(secondaryName ?? "-")}</div>
    </td>
    <td>
      <div class="cell-title">${escapeHtml(agent.ip_address)}</div>
      <div class="cell-sub">Port ${Number.isFinite(agent.ollama_port) ? escapeHtml(agent.ollama_port) : "-"}</div>
    </td>
    <td>${statusLabel}</td>
    <td>${formatDuration(agent.uptime_seconds)}</td>
    <td>
      <div class="cell-title">${cpuDisplay}</div>
      ${cpuGpuSub}
    </td>
    <td>
      <div class="cell-title">${memoryDisplay}</div>
      ${memoryGpuSub}
    </td>
    <td>${agent.active_requests}</td>
    <td>
      <div class="cell-title">${agent.total_requests}</div>
      <div class="cell-sub">
        成功 ${agent.successful_requests} / 失敗 ${agent.failed_requests}
      </div>
    </td>
    <td>${formatAverage(agent.average_response_time_ms)}</td>
    <td>
      <div class="cell-title">${escapeHtml(primaryModelDisplay)}</div>
      ${modelSub}
    </td>
    <td>
      <div class="cell-title">${formatTimestamp(agent.last_seen)}</div>
      <div class="cell-sub">${metricsDetail}</div>
    </td>
    <td>
      <button type="button" data-agent-id="${agent.id}">詳細</button>
    </td>
  `;
  syncAgentRowSelection(row, agent.id);

  return row;
}

function syncAgentRowSelection(row, agentId) {
  const checkbox = row.querySelector('input[data-agent-id]');
  if (!checkbox) return;
  const shouldCheck = state.selection.has(agentId);
  if (checkbox.checked !== shouldCheck) {
    checkbox.checked = shouldCheck;
  }
}

function buildPlaceholderRow(message) {
  const row = document.createElement("tr");
  row.className = "empty-row";
  row.innerHTML = `<td colspan="13">${escapeHtml(message)}</td>`;
  return row;
}

function getAgentSignature(agent) {
  return [
    agent.machine_name ?? "",
    agent.custom_name ?? "",
    agent.ip_address ?? "",
    agent.ollama_version ?? "",
    agent.status ?? "",
    agent.uptime_seconds ?? 0,
    agent.cpu_usage ?? 0,
    agent.memory_usage ?? 0,
    agent.gpu_usage ?? 0,
    agent.gpu_memory_usage ?? 0,
    agent.gpu_capability_score ?? "",
    agent.gpu_model_name ?? "",
    agent.gpu_compute_capability ?? "",
    agent.active_requests ?? 0,
    agent.total_requests ?? 0,
    agent.successful_requests ?? 0,
    agent.failed_requests ?? 0,
    agent.average_response_time_ms ?? "",
    agent.last_seen ?? "",
    agent.metrics_last_updated_at ?? "",
    agent.metrics_stale ? 1 : 0,
    getModelList(agent).join("|") ?? "",
  ].join("|");
}

function buildPageSignature(pageSlice) {
  return pageSlice.map((agent) => `${agent.id}:${getAgentSignature(agent)}`).join("|");
}

function buildSelectionSignature() {
  return Array.from(state.selection).sort().join("|");
}

function getDisplayName(agent) {
  const custom = typeof agent.custom_name === "string" ? agent.custom_name.trim() : "";
  if (custom) {
    return custom;
  }
  return agent.machine_name ?? "-";
}

function getModelList(agent) {
  if (!agent) return [];
  const list = Array.isArray(agent.loaded_models) ? agent.loaded_models : [];
  return list
    .map((model) => (typeof model === "string" ? model.trim() : ""))
    .filter((model) => model.length);
}

function filterAgent(agent, statusFilter, query) {
  if (statusFilter === "online" && agent.status !== "online") {
    return false;
  }
  if (statusFilter === "offline" && agent.status !== "offline") {
    return false;
  }

  if (!query) {
    return true;
  }

  const machine = (agent.machine_name ?? "").toLowerCase();
  const ip = (agent.ip_address ?? "").toLowerCase();
  const custom = (agent.custom_name ?? "").toLowerCase();
  const models = getModelList(agent).join(" ").toLowerCase();
  return (
    machine.includes(query) || ip.includes(query) || custom.includes(query) || models.includes(query)
  );
}

function getFilteredAgents() {
  return state.agents.filter((agent) =>
    filterAgent(agent, state.filterStatus, state.filterQuery),
  );
}

function sortAgents(agents, key, order) {
  const multiplier = order === "desc" ? -1 : 1;
  const safe = [...agents];
  safe.sort((a, b) => multiplier * compareAgents(a, b, key));
  return safe;
}

function compareAgents(a, b, key) {
  switch (key) {
    case "machine":
      return localeCompare(getDisplayName(a), getDisplayName(b));
    case "ip":
      return localeCompare(a.ip_address, b.ip_address);
    case "status":
      return localeCompare(a.status, b.status);
    case "uptime":
      return numericCompare(a.uptime_seconds, b.uptime_seconds);
    case "total":
      return numericCompare(a.total_requests, b.total_requests);
    default:
      return 0;
  }
}

function localeCompare(a, b) {
  return String(a ?? "").localeCompare(String(b ?? ""), "ja");
}

function numericCompare(a, b) {
  return Number(a ?? 0) - Number(b ?? 0);
}

function updateSortIndicators() {
  document.querySelectorAll("th[data-sort]").forEach((header) => {
    const indicator = header.querySelector(".sort-indicator");
    if (!indicator) return;

    if (header.dataset.sort === state.sortKey) {
      header.classList.add("sortable--active");
      indicator.textContent = state.sortOrder === "asc" ? "▲" : "▼";
    } else {
      header.classList.remove("sortable--active");
      indicator.textContent = "–";
    }
  });
}

function paginate(list, page, pageSize) {
  const start = (page - 1) * pageSize;
  return list.slice(start, start + pageSize);
}

function calculateTotalPages(length) {
  const total = length ?? getFilteredAgents().length;
  if (total === 0) return 1;
  return Math.ceil(total / state.pageSize);
}

function updatePagination(totalPages) {
  if (!paginationRefs.info) return;
  paginationRefs.info.textContent = `${state.currentPage} / ${totalPages || 1}`;
  if (paginationRefs.prev) {
    paginationRefs.prev.disabled = state.currentPage <= 1;
  }
  if (paginationRefs.next) {
    paginationRefs.next.disabled = state.currentPage >= (totalPages || 1);
  }
}

function openAgentModal(agent) {
  if (!modalRefs.modal) return;
  state.lastFocused = document.activeElement;
  state.selection = new Set([agent.id]);
  state.currentAgentId = agent.id;
  prepareAgentMetrics(agent.id);

  modalRefs.machineName.textContent = agent.machine_name ?? "-";
  modalRefs.ipAddress.textContent = agent.ip_address ?? "-";
  modalRefs.ollamaVersion.textContent = agent.ollama_version ?? "-";
  if (modalRefs.loadedModels) {
    const models = getModelList(agent);
    modalRefs.loadedModels.textContent = models.length ? models.join(", ") : "-";
  }
  modalRefs.uptime.textContent = formatDuration(agent.uptime_seconds);
  modalRefs.status.textContent = agent.status === "online" ? "オンライン" : "オフライン";
  modalRefs.lastSeen.textContent = formatTimestamp(agent.last_seen);
  modalRefs.totalRequests.textContent = agent.total_requests ?? 0;
  modalRefs.averageResponse.textContent = formatAverage(agent.average_response_time_ms);
  modalRefs.customName.value = agent.custom_name ?? "";
  modalRefs.tags.value = Array.isArray(agent.tags) ? agent.tags.join(", ") : "";
  modalRefs.notes.value = agent.notes ?? "";
  if (modalRefs.gpuUsage) {
    const gpuSummary = summarizeGpu(agent);
    const gpuModel = gpuSummary.primaryModel || 'GPU情報なし';
    const gpuCount = gpuSummary.totalCount > 1 ? ` (${gpuSummary.totalCount}枚)` : '';
    modalRefs.gpuUsage.textContent =
      typeof agent.gpu_usage === "number"
        ? formatPercentage(agent.gpu_usage)
        : `${gpuModel}${gpuCount} (メトリクス非対応)`;
  }
  if (modalRefs.gpuMemory) {
    const gpuSummary = summarizeGpu(agent);
    const gpuModel = gpuSummary.primaryModel || 'GPU情報なし';
    const gpuCount = gpuSummary.totalCount > 1 ? ` (${gpuSummary.totalCount}枚)` : '';
    modalRefs.gpuMemory.textContent =
      typeof agent.gpu_memory_usage === "number"
        ? formatPercentage(agent.gpu_memory_usage)
        : `${gpuModel}${gpuCount} (メトリクス非対応)`;
  }
  if (modalRefs.gpuCapabilityScore) {
    modalRefs.gpuCapabilityScore.textContent =
      typeof agent.gpu_capability_score === "number"
        ? agent.gpu_capability_score.toString()
        : "-";
  }
  if (modalRefs.gpuModel) {
    modalRefs.gpuModel.textContent = agent.gpu_model_name ?? "-";
  }
  if (modalRefs.gpuCompute) {
    modalRefs.gpuCompute.textContent = agent.gpu_compute_capability ?? "-";
  }

  const cached = state.agentMetricsCache.get(agent.id);
  if (cached && Date.now() - cached.fetchedAt.getTime() < 10_000) {
    updateAgentMetrics(cached.data);
  } else {
    loadAgentMetrics(agent.id);
  }

  modalRefs.modal.classList.remove("hidden");
  modalRefs.modal.setAttribute("tabindex", "-1");
  window.requestAnimationFrame(() => modalRefs.close.focus());
}

function closeAgentModal() {
  if (!modalRefs.modal) return;
  modalRefs.modal.classList.add("hidden");
  if (state.agentMetricsAbortController) {
    state.agentMetricsAbortController.abort();
    state.agentMetricsAbortController = null;
  }
  if (state.lastFocused && typeof state.lastFocused.focus === "function") {
    state.lastFocused.focus();
  }
  state.currentAgentId = null;
  destroyAgentMetricsChart();
}

function prepareAgentMetrics(agentId) {
  if (state.agentMetricsAbortController) {
    state.agentMetricsAbortController.abort();
  }
  state.agentMetricsAbortController = null;
  state.agentMetricsSignature = "";
  destroyAgentMetricsChart();
  setAgentMetricsStatus("メトリクスを読み込み中…");
  if (modalRefs.metricsCanvas) {
    modalRefs.metricsCanvas.dataset.agentId = agentId;
  }
}

async function loadAgentMetrics(agentId) {
  const controller = new AbortController();
  state.agentMetricsAbortController = controller;
  try {
    const metrics = await fetchJson(`/api/dashboard/metrics/${agentId}`, {
      signal: controller.signal,
    });
    if (controller.signal.aborted) {
      return;
    }
    state.agentMetricsAbortController = null;
    state.agentMetricsCache.set(agentId, { data: metrics, fetchedAt: new Date() });
    updateAgentMetrics(metrics);
  } catch (error) {
    if (controller.signal?.aborted) {
      return;
    }
    state.agentMetricsAbortController = null;
    destroyAgentMetricsChart();
    setAgentMetricsStatus(
      `メトリクスの取得に失敗しました: ${error?.message ?? error}`,
      { isError: true },
    );
  }
}

function updateAgentMetrics(metrics) {
  const array = Array.isArray(metrics)
    ? metrics.slice(Math.max(metrics.length - AGENT_METRICS_LIMIT, 0))
    : [];

  if (!array.length) {
    state.agentMetricsSignature = "";
    destroyAgentMetricsChart();
    setAgentMetricsStatus("メトリクスはまだありません");
    return;
  }

  const signature = buildAgentMetricsSignature(array);
  if (signature === state.agentMetricsSignature && agentMetricsChart) {
    setAgentMetricsStatus(buildAgentMetricsSummary(array));
    return;
  }

  state.agentMetricsSignature = signature;

  const canvas = modalRefs.metricsCanvas;
  if (!canvas) return;

  const labels = array.map((point) => formatMetricLabel(new Date(point.timestamp)));
  const cpu = array.map((point) => toNullableNumber(point.cpu_usage));
  const memory = array.map((point) => toNullableNumber(point.memory_usage));
  const gpu = array.map((point) => toNullableNumber(point.gpu_usage));
  const gpuMemory = array.map((point) => toNullableNumber(point.gpu_memory_usage));

  const datasets = [];
  if (datasetHasValues(cpu)) {
    datasets.push({
      key: "cpu",
      label: "CPU使用率",
      data: cpu,
      borderColor: "rgba(59, 130, 246, 0.85)",
      backgroundColor: "rgba(59, 130, 246, 0.12)",
    });
  }
  if (datasetHasValues(memory)) {
    datasets.push({
      key: "memory",
      label: "メモリ使用率",
      data: memory,
      borderColor: "rgba(168, 85, 247, 0.85)",
      backgroundColor: "rgba(168, 85, 247, 0.12)",
    });
  }
  if (datasetHasValues(gpu)) {
    datasets.push({
      key: "gpu",
      label: "GPU使用率",
      data: gpu,
      borderColor: "rgba(34, 197, 94, 0.85)",
      backgroundColor: "rgba(34, 197, 94, 0.12)",
    });
  }
  if (datasetHasValues(gpuMemory)) {
    datasets.push({
      key: "gpu-memory",
      label: "GPUメモリ使用率",
      data: gpuMemory,
      borderColor: "rgba(248, 113, 113, 0.85)",
      backgroundColor: "rgba(248, 113, 113, 0.12)",
    });
  }

  if (!datasets.length) {
    destroyAgentMetricsChart();
    setAgentMetricsStatus("メトリクスは記録されていますが数値を取得できませんでした");
    return;
  }

  const shouldRecreate =
    !agentMetricsChart ||
    agentMetricsChart.data.datasets.length !== datasets.length ||
    datasets.some((dataset, index) => agentMetricsChart.data.datasets[index]?.label !== dataset.label);

  if (shouldRecreate) {
    destroyAgentMetricsChart();
    agentMetricsChart = new Chart(canvas, {
      type: "line",
      data: {
        labels,
        datasets: datasets.map((dataset) => ({
          label: dataset.label,
          data: dataset.data,
          borderColor: dataset.borderColor,
          backgroundColor: dataset.backgroundColor,
          fill: true,
          pointRadius: 0,
          tension: 0.25,
        })),
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        interaction: {
          mode: "index",
          intersect: false,
        },
        scales: {
          x: {
            ticks: {
              color: "var(--text-subtle)",
              maxRotation: 0,
            },
            grid: {
              color: "rgba(148, 163, 184, 0.08)",
            },
          },
          y: {
            suggestedMin: 0,
            suggestedMax: 100,
            ticks: {
              color: "var(--text-subtle)",
              callback: (value) => `${value}%`,
            },
            grid: {
              color: "rgba(148, 163, 184, 0.08)",
            },
          },
        },
        plugins: {
          legend: {
            labels: {
              color: "var(--text-subtle)",
            },
          },
          tooltip: {
            callbacks: {
              label(context) {
                const value = context.parsed.y;
                if (value == null) {
                  return `${context.dataset.label}: -`;
                }
                return `${context.dataset.label}: ${value.toFixed(1)}%`;
              },
            },
          },
        },
      },
    });
  } else {
    agentMetricsChart.data.labels = labels;
    datasets.forEach((dataset, index) => {
      agentMetricsChart.data.datasets[index].data = dataset.data;
      agentMetricsChart.data.datasets[index].label = dataset.label;
    });
    agentMetricsChart.update("none");
  }

  setAgentMetricsStatus(buildAgentMetricsSummary(array));
}

function destroyAgentMetricsChart() {
  if (agentMetricsChart) {
    agentMetricsChart.destroy();
    agentMetricsChart = null;
  }
}

function setAgentMetricsStatus(message, { isError = false } = {}) {
  if (!modalRefs.metricsStatus) return;
  modalRefs.metricsStatus.textContent = message;
  modalRefs.metricsStatus.classList.toggle("is-error", isError);
}

function datasetHasValues(values) {
  return values.some((value) => typeof value === "number" && !Number.isNaN(value));
}

function buildAgentMetricsSummary(metrics) {
  const latest = metrics[metrics.length - 1];
  const latestTime = formatMetricLabel(new Date(latest.timestamp));
  const parts = [
    `CPU ${formatPercentage(latest.cpu_usage)}`,
    `メモリ ${formatPercentage(latest.memory_usage)}`,
    `GPU ${formatPercentage(latest.gpu_usage)}`,
    `GPUメモリ ${formatPercentage(latest.gpu_memory_usage)}`,
  ];
  return `データ点: ${metrics.length} / 最新 ${latestTime} | ${parts.join(" / ")}`;
}

function buildAgentMetricsSignature(metrics) {
  return metrics
    .map((point) => {
      const cpu = typeof point.cpu_usage === "number" ? point.cpu_usage.toFixed(2) : "-";
      const memory =
        typeof point.memory_usage === "number" ? point.memory_usage.toFixed(2) : "-";
      const gpu = typeof point.gpu_usage === "number" ? point.gpu_usage.toFixed(2) : "-";
      const gpuMemory =
        typeof point.gpu_memory_usage === "number"
          ? point.gpu_memory_usage.toFixed(2)
          : "-";
      const ts = point.timestamp ?? "";
      return `${ts}:${cpu}:${memory}:${gpu}:${gpuMemory}`;
    })
    .join("|");
}

function toNullableNumber(value) {
  return typeof value === "number" && Number.isFinite(value) ? Number(value.toFixed(2)) : null;
}

async function saveAgentSettings(agentId) {
  const tags = modalRefs.tags.value
    .split(",")
    .map((tag) => tag.trim())
    .filter(Boolean);

  const payload = {
    custom_name: modalRefs.customName.value.trim() || null,
    tags,
    notes: modalRefs.notes.value.trim() || null,
  };

  try {
    const response = await fetch(`/api/agents/${agentId}/settings`, {
      method: "PUT",
      headers: {
        "Content-Type": "application/json",
        Accept: "application/json",
      },
      body: JSON.stringify(payload),
    });

    if (!response.ok) {
      throw new Error(`${response.status} ${response.statusText}`);
    }

    return response.json();
  } catch (error) {
    console.error("Failed to save agent settings:", error);
    showError(`設定の保存に失敗しました: ${error.message}`);
    throw error;
  }
}

async function deleteAgent(agentId) {
  try {
    const response = await fetch(`/api/agents/${agentId}`, {
      method: "DELETE",
      headers: {
        Accept: "application/json",
      },
    });

    if (!response.ok) {
      throw new Error(`${response.status} ${response.statusText}`);
    }
  } catch (error) {
    showError(`エージェントの削除に失敗しました: ${error.message}`);
    throw error;
  }
}

async function disconnectAgent(agentId) {
  try {
    const response = await fetch(`/api/agents/${agentId}/disconnect`, {
      method: "POST",
      headers: {
        Accept: "application/json",
      },
    });

    if (!response.ok) {
      throw new Error(`${response.status} ${response.statusText}`);
    }
  } catch (error) {
    showError(`強制切断に失敗しました: ${error.message}`);
    throw error;
  }
}

function downloadJson(data, filename) {
  const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
  triggerDownload(blob, filename);
}

function downloadCsv(data, filename) {
  const headers = [
    "id",
    "display_name",
    "machine_name",
    "ip_address",
    "ollama_version",
    "status",
    "cpu_usage",
    "memory_usage",
    "gpu_usage",
    "gpu_memory_usage",
    "registered_at",
    "last_seen",
    "loaded_models",
    "tags",
  ];

  const rows = data.map((agent) => {
    const models = getModelList(agent).join("|");
    return [
      agent.id,
      getDisplayName(agent),
      agent.machine_name ?? "",
      agent.ip_address ?? "",
      agent.ollama_version ?? "",
      agent.status ?? "",
      agent.cpu_usage ?? "",
      agent.memory_usage ?? "",
      agent.gpu_usage ?? "",
      agent.gpu_memory_usage ?? "",
      agent.registered_at ?? "",
      agent.last_seen ?? "",
      models,
      Array.isArray(agent.tags) ? agent.tags.join("|") : "",
    ]
      .map((value) => `"${String(value).replace(/"/g, '""')}"`)
      .join(",");
  });

  const csv = [headers.join(","), ...rows].join("\n");
  const blob = new Blob([csv], { type: "text/csv;charset=utf-8;" });
  triggerDownload(blob, filename);
}

function triggerDownload(blob, filename) {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
  URL.revokeObjectURL(url);
}

function setConnectionStatus(mode) {
  const pill = document.getElementById("connection-status");
  if (!pill) return;

  pill.classList.remove("status-pill--online", "status-pill--offline");

  const labelMap = {
    loading: "接続状態: 更新中…",
    updating: "接続状態: 更新中…",
    online: "接続状態: 正常",
    offline: "接続状態: 切断",
  };

  pill.textContent = labelMap[mode] ?? "接続状態: -";

  if (mode === "online") {
    pill.classList.add("status-pill--online");
  } else if (mode === "offline") {
    pill.classList.add("status-pill--offline");
  }
}

function updateLastRefreshed(date, serverDate = null) {
  const label = document.getElementById("last-refreshed");
  if (!label) return;
  const clientText = formatDate(date);
  const serverText =
    serverDate instanceof Date && !Number.isNaN(serverDate.getTime())
      ? ` / サーバー: ${formatDate(serverDate)}`
      : "";
  label.textContent = `最終更新: ${clientText}${serverText}`;
}

function showError(message) {
  const banner = document.getElementById("error-banner");
  if (!banner) return;
  banner.textContent = message;
  banner.classList.remove("hidden");
}

function hideError() {
  const banner = document.getElementById("error-banner");
  if (!banner) return;
  banner.classList.add("hidden");
  banner.textContent = "";
}

function formatDuration(seconds) {
  if (typeof seconds !== "number" || Number.isNaN(seconds)) {
    return "-";
  }

  const abs = Math.max(0, Math.floor(seconds));
  const days = Math.floor(abs / 86400);
  const hours = Math.floor((abs % 86400) / 3600);
  const minutes = Math.floor((abs % 3600) / 60);

  if (days > 0) {
    return `${days}日${hours}時間`;
  }
  if (hours > 0) {
    return `${hours}時間${minutes}分`;
  }
  if (minutes > 0) {
    return `${minutes}分`;
  }
  return `${abs}秒`;
}

function formatPercentage(value) {
  if (typeof value !== "number" || Number.isNaN(value)) {
    return "-";
  }
  return `${value.toFixed(1)}%`;
}

function formatAverage(value) {
  if (typeof value !== "number" || Number.isNaN(value)) {
    return "-";
  }
  if (value >= 1000) {
    return `${(value / 1000).toFixed(2)} s`;
  }
  return `${value.toFixed(0)} ms`;
}

function formatTimestamp(isoString) {
  if (!isoString) {
    return "-";
  }
  return formatDate(new Date(isoString));
}

function formatDate(date) {
  if (!(date instanceof Date) || Number.isNaN(date.getTime())) {
    return "-";
  }

  return date.toLocaleString("ja-JP", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function formatMetricLabel(date) {
  if (!(date instanceof Date) || Number.isNaN(date.getTime())) {
    return "-";
  }

  return date.toLocaleTimeString("ja-JP", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function formatHistoryLabel(date) {
  if (!(date instanceof Date) || Number.isNaN(date.getTime())) {
    return "-";
  }

  return date.toLocaleTimeString("ja-JP", {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function alignDateToMinute(date) {
  if (!(date instanceof Date) || Number.isNaN(date.getTime())) {
    return new Date();
  }
  const copy = new Date(date.getTime());
  copy.setSeconds(0, 0);
  return copy;
}

function escapeHtml(value) {
  if (value == null) {
    return "-";
  }
  return String(value)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}
