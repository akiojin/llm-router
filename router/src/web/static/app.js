const REFRESH_INTERVAL_MS = 5000;
const PERFORMANCE_THRESHOLDS = Object.freeze({
  fetch: 2000,
  render: 100,
  backend: 100,
});
const NODE_METRICS_LIMIT = 120;
const LOG_ENTRY_LIMIT = 200;
const MODAL_LOG_ENTRY_LIMIT = 100;

const state = {
  nodes: [],
  stats: null,
  history: [],
  filterStatus: "all",
  filterQuery: "",
  sortKey: "machine",
  sortOrder: "asc",
  lastFocused: null,
  selection: new Set(),
  selectAll: false,
  currentNodeId: null,
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
  nodeMetricsSignature: "",
  nodeMetricsCache: new Map(),
  nodeMetricsAbortController: null,
  fallbackNotified: false,
  currentTab: "dashboard",
  logs: {
    coordinator: [],
    coordinatorPath: null,
    node: [],
    nodePath: null,
    selectedNodeId: null,
    coordinatorFetched: false,
    nodeFetched: false,
    loadingCoordinator: false,
    loadingNode: false,
    coordinatorError: null,
    nodeError: null,
  },
  modalLog: {
    entries: [],
    path: null,
    loading: false,
    error: null,
    fetchedNodeId: null,
  },
};

let requestsChart = null;
let nodeMetricsChart = null;
const modalRefs = {
  modal: null,
  close: null,
  ok: null,
  save: null,
  delete: null,
  machineName: null,
  ipAddress: null,
  runtimeVersion: null,
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
  logSection: null,
  logViewer: null,
  logStatus: null,
  logPath: null,
  logRefresh: null,
};
const paginationRefs = { prev: null, next: null, info: null };
const logRefs = {
  coordinatorList: null,
  coordinatorPath: null,
  coordinatorStatus: null,
  coordinatorRefresh: null,
  nodeList: null,
  nodePath: null,
  nodeStatus: null,
  nodeSelect: null,
  nodeRefresh: null,
};

document.addEventListener("DOMContentLoaded", () => {
  const refreshButton = document.getElementById("refresh-button");
  const statusSelect = document.getElementById("filter-status");
  const queryInput = document.getElementById("filter-query");
  const sortableHeaders = document.querySelectorAll("th[data-sort]");
  const selectAllCheckbox = document.getElementById("select-all");
  const exportJsonButton = document.getElementById("export-json");
  const exportCsvButton = document.getElementById("export-csv");
  const modal = document.getElementById("node-modal");
  const modalClose = document.getElementById("node-modal-close");
  const modalOk = document.getElementById("node-modal-ok");
  const modalSave = document.getElementById("node-modal-save");
  const modalDelete = document.getElementById("node-modal-delete");
  const modalDisconnect = document.getElementById("node-modal-disconnect");
  const chatOpen = document.getElementById("chat-open");
  const chatModal = document.getElementById("chat-modal");
  const chatClose = document.getElementById("chat-close");
  const chatIframe = document.getElementById("chat-iframe");
  const tbody = document.getElementById("nodes-body");

  // リロード直後に詳細モーダルが開いたままにならないよう、確実に非表示へ初期化
  modal?.classList.add("hidden");
  document.getElementById("request-modal")?.classList.add("hidden");
  chatModal?.classList.add("hidden");

  initTabs();

  paginationRefs.prev = document.getElementById("page-prev");
  paginationRefs.next = document.getElementById("page-next");
  paginationRefs.info = document.getElementById("page-info");
  state.selectAllCheckbox = selectAllCheckbox;
  state.performanceIndicator = document.getElementById("refresh-metrics");
  updatePerformanceIndicator();

  logRefs.coordinatorList = document.getElementById("logs-coordinator-list");
  logRefs.coordinatorPath = document.getElementById("logs-coordinator-path");
  logRefs.coordinatorStatus = document.getElementById("logs-coordinator-status");
  logRefs.coordinatorRefresh = document.getElementById("logs-coordinator-refresh");
  logRefs.nodeList = document.getElementById("logs-node-list");
  logRefs.nodePath = document.getElementById("logs-node-path");
  logRefs.nodeStatus = document.getElementById("logs-node-status");
  logRefs.nodeSelect = document.getElementById("logs-node-select");
  logRefs.nodeRefresh = document.getElementById("logs-node-refresh");
  initLogControls();
  renderCoordinatorLogs();
  renderNodeLogs();
  renderLogsNodeOptions();
  initModalLogControls();

  Object.assign(modalRefs, {
    modal,
    close: modalClose,
    ok: modalOk,
    machineName: document.getElementById("detail-machine-name"),
    ipAddress: document.getElementById("detail-ip-address"),
    runtimeVersion: document.getElementById("detail-runtime-version"),
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
    metricsStatus: document.getElementById("node-metrics-status"),
    metricsCanvas: document.getElementById("node-metrics-chart"),
    logSection: document.getElementById("node-log-section"),
    logViewer: document.getElementById("node-log-viewer"),
    logStatus: document.getElementById("node-log-status"),
    logPath: document.getElementById("node-log-path"),
    logRefresh: document.getElementById("node-log-refresh"),
  });

  refreshButton.addEventListener("click", () => refreshData({ manual: true }));
  statusSelect.addEventListener("change", (event) => {
    state.filterStatus = event.target.value;
    state.currentPage = 1;
    renderAgents();
  });
  chatOpen?.addEventListener("click", () => {
    if (!chatModal) return;
    chatModal.classList.remove("hidden");
    document.body.classList.add("body--modal-open");
    chatIframe?.focus();
  });
  const closeChat = () => {
    chatModal?.classList.add("hidden");
    document.body.classList.remove("body--modal-open");
  };
  chatClose?.addEventListener("click", closeChat);
  chatModal?.addEventListener("click", (event) => {
    if (event.target?.dataset?.chatClose !== undefined) {
      closeChat();
    }
  });
  window.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && !chatModal?.classList.contains("hidden")) {
      closeChat();
    }
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
      const filtered = state.nodes.filter((node) =>
        filterNode(node, state.filterStatus, state.filterQuery),
      );
      state.selection = new Set(filtered.map((node) => node.id));
    } else {
      state.selection.clear();
    }
    renderNodes();
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
      renderNodes();
    });
  });

  updateSortIndicators();

  paginationRefs.prev?.addEventListener("click", () => {
    if (state.currentPage > 1) {
      state.currentPage -= 1;
      renderNodes();
    }
  });

  paginationRefs.next?.addEventListener("click", () => {
    const totalPages = calculateTotalPages();
    if (state.currentPage < totalPages) {
      state.currentPage += 1;
      renderNodes();
    }
  });

  exportJsonButton.addEventListener("click", () => {
    const data = getFilteredNodes();
    downloadJson(data, "nodes.json");
  });

  exportCsvButton.addEventListener("click", () => {
    const data = getFilteredNodes();
    downloadCsv(data, "nodes.csv");
  });

  tbody.addEventListener("click", (event) => {
    const rowCheckbox = event.target.closest("input[data-node-id]");
    if (rowCheckbox) {
      const nodeId = rowCheckbox.dataset.nodeId;
      if (rowCheckbox.checked) {
        state.selection.add(nodeId);
      } else {
        state.selection.delete(nodeId);
        state.selectAll = false;
        selectAllCheckbox.checked = false;
      }
      return;
    }
    const button = event.target.closest("button[data-node-id]");
    if (!button) return;
    const nodeId = button.dataset.nodeId;
    const node = state.nodes.find((item) => item.id === nodeId);
    if (node) {
      openNodeModal(node);
    }
  });

  const closeModal = () => closeNodeModal();
  modalClose.addEventListener("click", closeModal);
  modalOk.addEventListener("click", closeModal);
  modalSave.addEventListener("click", async () => {
    if (!state.currentNodeId) return;
    const nodeId = state.currentNodeId;
    try {
      const updated = await saveNodeSettings(nodeId);
      if (updated && updated.id) {
        state.nodes = state.nodes.map((node) =>
          node.id === updated.id ? { ...node, ...updated } : node,
        );
        closeNodeModal();
        renderNodes();
      }
    } catch (error) {
      console.error("Failed to persist node settings", error);
    }
  });
  modalDelete.addEventListener("click", async () => {
    if (!state.currentNodeId) return;
    const nodeId = state.currentNodeId;
    const node = state.nodes.find((item) => item.id === nodeId);
    const name = node ? getDisplayName(node) : "target";
    if (!window.confirm(`Delete ${name}?`)) {
      return;
    }

    try {
      await deleteNode(nodeId);
      state.nodes = state.nodes.filter((item) => item.id !== nodeId);
      state.selection.delete(nodeId);
      closeNodeModal();
      renderNodes();
    } catch (error) {
      console.error("Failed to delete node", error);
    }
  });
  modalDisconnect.addEventListener("click", async () => {
    if (!state.currentNodeId) return;
    const nodeId = state.currentNodeId;
    try {
      await disconnectNode(nodeId);
      const node = state.nodes.find((item) => item.id === nodeId);
      if (node) {
        node.status = "offline";
      }
      renderNodes();
    } catch (error) {
      console.error("Failed to disconnect node", error);
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
    await applyOverviewData(overview);
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

async function applyOverviewData(overview) {
  state.nodes = Array.isArray(overview.nodes) ? overview.nodes : [];
  state.stats = overview.stats ?? null;
  state.history = Array.isArray(overview.history) ? overview.history : [];
  const generatedAt =
    typeof overview.generated_at === "string" ? new Date(overview.generated_at) : null;
  state.metrics.generatedAt = generatedAt;

  renderStats();
  renderNodes();
  renderHistory();
  renderLogsNodeOptions();
  // タブレス表示のため常にログ更新を試みる
  maybeRefreshLogs();
  hideError();
  setConnectionStatus("online");
  updateLastRefreshed(new Date(), generatedAt);

}

async function fetchLegacyOverview() {
  const [nodes, stats, history] = await Promise.all([
    fetchJson("/api/dashboard/nodes"),
    fetchJson("/api/dashboard/stats"),
    fetchJson("/api/dashboard/request-history"),
  ]);

  return { nodes, stats, history };
}

async function fetchOverview() {
  return fetchJson("/api/dashboard/overview");
}

function handleRefreshFailure(error) {
  console.error("Dashboard refresh failed:", error);
  showError(`Failed to fetch dashboard data: ${error?.message ?? error}`);
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
    `Fetch: ${fetchMs == null ? "-" : `${fetchMs} ms`}`,
    `Render: ${renderMs == null ? "-" : `${renderMs} ms`}`,
    `Server: ${backendMs == null ? "-" : `${backendMs} ms`}`,
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
    "total-agents": state.stats.total_nodes,
    "online-agents": state.stats.online_nodes,
    "offline-agents": state.stats.offline_nodes,
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

function renderNodes() {
  const tbody = document.getElementById("nodes-body");
  if (!tbody) return;

  if (!state.nodes.length) {
    state.rowCache.clear();
    state.renderSnapshot = null;
    state.selectAll = false;
    if (state.selectAllCheckbox) {
      state.selectAllCheckbox.checked = false;
    }
    tbody.replaceChildren(buildPlaceholderRow("No nodes registered yet"));
    updatePagination(0);
    return;
  }

  const filtered = getFilteredNodes();

  if (!filtered.length) {
    state.renderSnapshot = null;
    state.selectAll = false;
    if (state.selectAllCheckbox) {
      state.selectAllCheckbox.checked = false;
    }
    tbody.replaceChildren(buildPlaceholderRow("No nodes match the filter criteria"));
    updatePagination(0);
    return;
  }

  state.selectAll = filtered.every((node) => state.selection.has(node.id));
  if (state.selectAllCheckbox) {
    state.selectAllCheckbox.checked = state.selectAll;
  }

  const sorted = sortNodes(filtered, state.sortKey, state.sortOrder);
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

  pageSlice.forEach((node) => {
    const signature = getNodeSignature(node);
    const cached = state.rowCache.get(node.id);
    let row = cached?.row;
    if (!row) {
      row = document.createElement("tr");
    }

    if (!cached || cached.signature !== signature) {
      buildNodeRow(node, row);
      state.rowCache.set(node.id, { row: row, signature });
    } else {
      syncNodeRowSelection(row, node.id);
      row.classList.toggle("node-offline", node.status === "offline");
    }

    fragment.appendChild(row);
  });

  tbody.replaceChildren(fragment);

  if (state.rowCache.size > state.nodes.length) {
    const knownIds = new Set(state.nodes.map((node) => node.id));
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
            label: "Success Requests",
            data: success,
            tension: 0.3,
            borderColor: "rgba(59, 130, 246, 0.9)",
            backgroundColor: "rgba(59, 130, 246, 0.15)",
            fill: true,
            pointRadius: 0,
          },
          {
            label: "Failed Requests",
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

function summarizeGpu(node) {
  const devices = Array.isArray(node.gpu_devices) ? node.gpu_devices : [];
  const totalFromDevices = devices.reduce(
    (sum, device) => sum + (Number(device?.count) || 0),
    0
  );
  const fallbackCount = typeof node.gpu_count === "number" ? node.gpu_count : 0;
  const totalCount = totalFromDevices || fallbackCount;
  const primaryModel = devices.length > 0 && devices[0]?.model
    ? devices[0].model
    : node.gpu_model;

  return {
    devices,
    totalCount,
    primaryModel,
  };
}

function buildNodeRow(node, row = document.createElement("tr")) {
  row.dataset.nodeId = node.id;
  row.classList.toggle("node-offline", node.status === "offline");

  const displayName = getDisplayName(node);
  const secondaryName = node.custom_name
    ? node.machine_name
    : node.runtime_version || node.machine_name;

  const statusLabel =
    node.status === "online"
      ? node.initializing
        ? `<span class="badge badge--warming">Warming up${formatReadyProgress(node.ready_models)}</span>`
        : '<span class="badge badge--online">Online</span>'
      : '<span class="badge badge--offline">Offline</span>';

  const metricsBadge = node.metrics_stale
    ? '<span class="badge badge--stale">STALE</span>'
    : "";
  const metricsTimestamp = formatTimestamp(node.metrics_last_updated_at);
  const metricsDetail = metricsBadge ? `${metricsBadge} ${metricsTimestamp}` : metricsTimestamp;

  const cpuDisplay = formatPercentage(node.cpu_usage);
  const gpuSummary = summarizeGpu(node);
  const gpuModelDisplay = gpuSummary.primaryModel
    ? `${escapeHtml(gpuSummary.primaryModel)}${
        gpuSummary.totalCount > 1 ? ` (${gpuSummary.totalCount} GPUs)` : ''
      }`
    : 'Fetching GPU info';
  // GPU capability score display
  const gpuScoreText =
    typeof node.gpu_capability_score === "number"
      ? ` / Score ${node.gpu_capability_score}`
      : "";
  const cpuGpuSub =
    typeof node.gpu_usage === "number"
      ? `<div class="cell-sub">GPU ${formatPercentage(node.gpu_usage)} (${gpuModelDisplay})${gpuScoreText}</div>`
      : `<div class="cell-sub">${gpuModelDisplay}${gpuScoreText}</div>`;
  const memoryDisplay = formatPercentage(node.memory_usage);
  const memoryGpuSub =
    typeof node.gpu_memory_usage === "number"
      ? `<div class="cell-sub">GPU ${formatPercentage(node.gpu_memory_usage)} (${gpuModelDisplay})</div>`
      : `<div class="cell-sub">${gpuModelDisplay}</div>`;
  const readyText =
    node.initializing || node.ready_models
      ? `<div class="cell-sub ready-progress">${formatReadyProgress(node.ready_models)}</div>`
      : "";

  row.innerHTML = `
    <td>
      <input
        type="checkbox"
        data-node-id="${node.id}"
        ${state.selection.has(node.id) ? "checked" : ""}
        aria-label="Select ${escapeHtml(node.machine_name)}"
      />
    </td>
    <td>
      <div class="cell-title">${escapeHtml(displayName)}</div>
      <div class="cell-sub">${escapeHtml(secondaryName ?? "-")}</div>
    </td>
    <td>
      <div class="cell-title">${escapeHtml(node.ip_address)}</div>
      <div class="cell-sub">Port ${Number.isFinite(node.runtime_port) ? escapeHtml(node.runtime_port) : "-"}</div>
      ${readyText}
    </td>
    <td>${statusLabel}</td>
    <td>${formatDuration(node.uptime_seconds)}</td>
    <td>
      <div class="cell-title">${cpuDisplay}</div>
      ${cpuGpuSub}
    </td>
    <td>
      <div class="cell-title">${memoryDisplay}</div>
      ${memoryGpuSub}
    </td>
    <td>${node.active_requests}</td>
    <td>
      <div class="cell-title">${node.total_requests}</div>
      <div class="cell-sub">
        Success ${node.successful_requests} / Failed ${node.failed_requests}
      </div>
    </td>
    <td>${formatAverage(node.average_response_time_ms)}</td>
    <td>
      <div class="cell-title">${formatTimestamp(node.last_seen)}</div>
      <div class="cell-sub">${metricsDetail}</div>
    </td>
    <td>
      <button type="button" data-node-id="${node.id}">Details</button>
    </td>
  `;
  syncNodeRowSelection(row, node.id);

  return row;
}

function syncNodeRowSelection(row, nodeId) {
  const checkbox = row.querySelector('input[data-node-id]');
  if (!checkbox) return;
  const shouldCheck = state.selection.has(nodeId);
  if (checkbox.checked !== shouldCheck) {
    checkbox.checked = shouldCheck;
  }
}

function buildPlaceholderRow(message) {
  const row = document.createElement("tr");
  row.className = "empty-row";
  row.innerHTML = `<td colspan="12">${escapeHtml(message)}</td>`;
  return row;
}

function getNodeSignature(node) {
  return [
    node.machine_name ?? "",
    node.custom_name ?? "",
    node.ip_address ?? "",
    node.runtime_version ?? "",
    node.status ?? "",
    node.uptime_seconds ?? 0,
    node.cpu_usage ?? 0,
    node.memory_usage ?? 0,
    node.gpu_usage ?? 0,
    node.gpu_memory_usage ?? 0,
    node.gpu_capability_score ?? "",
    node.gpu_model_name ?? "",
    node.gpu_compute_capability ?? "",
    node.initializing ? 1 : 0,
    Array.isArray(node.ready_models) ? node.ready_models.join(":") : "",
    node.active_requests ?? 0,
    node.total_requests ?? 0,
    node.successful_requests ?? 0,
    node.failed_requests ?? 0,
    node.average_response_time_ms ?? "",
    node.last_seen ?? "",
    node.metrics_last_updated_at ?? "",
    node.metrics_stale ? 1 : 0,
  ].join("|");
}

function buildPageSignature(pageSlice) {
  return pageSlice.map((node) => `${node.id}:${getNodeSignature(node)}`).join("|");
}

function buildSelectionSignature() {
  return Array.from(state.selection).sort().join("|");
}

function getDisplayName(node) {
  const custom = typeof node.custom_name === "string" ? node.custom_name.trim() : "";
  if (custom) {
    return custom;
  }
  return node.machine_name ?? "-";
}


function filterNode(node, statusFilter, query) {
  if (statusFilter === "online" && node.status !== "online") {
    return false;
  }
  if (statusFilter === "offline" && node.status !== "offline") {
    return false;
  }

  if (!query) {
    return true;
  }

  const machine = (node.machine_name ?? "").toLowerCase();
  const ip = (node.ip_address ?? "").toLowerCase();
  const custom = (node.custom_name ?? "").toLowerCase();
  return (machine.includes(query) || ip.includes(query) || custom.includes(query));
}

function getFilteredNodes() {
  return state.nodes.filter((node) =>
    filterNode(node, state.filterStatus, state.filterQuery),
  );
}

function sortNodes(nodes, key, order) {
  const multiplier = order === "desc" ? -1 : 1;
  const safe = [...nodes];
  safe.sort((a, b) => multiplier * compareNodes(a, b, key));
  return safe;
}

function compareNodes(a, b, key) {
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
  const total = length ?? getFilteredNodes().length;
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

function openNodeModal(node) {
  if (!modalRefs.modal) return;
  state.lastFocused = document.activeElement;
  state.selection = new Set([node.id]);
  state.currentNodeId = node.id;
  prepareNodeMetrics(node.id);
  resetModalNodeLogs();

  modalRefs.machineName.textContent = node.machine_name ?? "-";
  modalRefs.ipAddress.textContent = node.ip_address ?? "-";
  modalRefs.runtimeVersion.textContent = node.runtime_version ?? "-";
  if (modalRefs.loadedModels) {
    const models = Array.isArray(node.loaded_models) ? node.loaded_models : [];

    modalRefs.loadedModels.textContent = models.length ? models.join(", ") : "-";
  }
  modalRefs.uptime.textContent = formatDuration(node.uptime_seconds);
  modalRefs.status.textContent = node.status === "online" ? "Online" : "Offline";
  modalRefs.lastSeen.textContent = formatTimestamp(node.last_seen);
  modalRefs.totalRequests.textContent = node.total_requests ?? 0;
  modalRefs.averageResponse.textContent = formatAverage(node.average_response_time_ms);
  modalRefs.customName.value = node.custom_name ?? "";
  modalRefs.tags.value = Array.isArray(node.tags) ? node.tags.join(", ") : "";
  modalRefs.notes.value = node.notes ?? "";
  if (modalRefs.gpuUsage) {
    const gpuSummary = summarizeGpu(node);
    const gpuModel = gpuSummary.primaryModel || 'No GPU info';
    const gpuCount = gpuSummary.totalCount > 1 ? ` (${gpuSummary.totalCount} GPUs)` : '';
    modalRefs.gpuUsage.textContent =
      typeof node.gpu_usage === "number"
        ? formatPercentage(node.gpu_usage)
        : `${gpuModel}${gpuCount} (Metrics not supported)`;
  }
  if (modalRefs.gpuMemory) {
    const gpuSummary = summarizeGpu(node);
    const gpuModel = gpuSummary.primaryModel || 'No GPU info';
    const gpuCount = gpuSummary.totalCount > 1 ? ` (${gpuSummary.totalCount} GPUs)` : '';
    modalRefs.gpuMemory.textContent =
      typeof node.gpu_memory_usage === "number"
        ? formatPercentage(node.gpu_memory_usage)
        : `${gpuModel}${gpuCount} (Metrics not supported)`;
  }
  if (modalRefs.gpuCapabilityScore) {
    modalRefs.gpuCapabilityScore.textContent =
      typeof node.gpu_capability_score === "number"
        ? node.gpu_capability_score.toString()
        : "-";
  }
  if (modalRefs.gpuModel) {
    modalRefs.gpuModel.textContent = node.gpu_model_name ?? "-";
  }
  if (modalRefs.gpuCompute) {
    modalRefs.gpuCompute.textContent = node.gpu_compute_capability ?? "-";
  }

  const cached = state.nodeMetricsCache.get(node.id);
  if (cached && Date.now() - cached.fetchedAt.getTime() < 10_000) {
    updateNodeMetrics(cached.data);
  } else {
    loadNodeMetrics(node.id);
  }

  modalRefs.modal.classList.remove("hidden");
  modalRefs.modal.setAttribute("tabindex", "-1");
  loadModalNodeLogs(node.id, { force: true });
  window.requestAnimationFrame(() => modalRefs.close.focus());
}

function closeNodeModal() {
  if (!modalRefs.modal) return;
  modalRefs.modal.classList.add("hidden");
  if (state.nodeMetricsAbortController) {
    state.nodeMetricsAbortController.abort();
    state.nodeMetricsAbortController = null;
  }
  if (state.lastFocused && typeof state.lastFocused.focus === "function") {
    state.lastFocused.focus();
  }
  state.currentNodeId = null;
  destroyNodeMetricsChart();
  resetModalNodeLogs();
}

function prepareNodeMetrics(nodeId) {
  if (state.nodeMetricsAbortController) {
    state.nodeMetricsAbortController.abort();
  }
  state.nodeMetricsAbortController = null;
  state.nodeMetricsSignature = "";
  destroyNodeMetricsChart();
  setNodeMetricsStatus("Loading metrics...");
  if (modalRefs.metricsCanvas) {
    modalRefs.metricsCanvas.dataset.nodeId = nodeId;
  }
}

async function loadNodeMetrics(nodeId) {
  const controller = new AbortController();
  state.nodeMetricsAbortController = controller;
  try {
    const metrics = await fetchJson(`/api/dashboard/metrics/${nodeId}`, {
      signal: controller.signal,
    });
    if (controller.signal.aborted) {
      return;
    }
    state.nodeMetricsAbortController = null;
    state.nodeMetricsCache.set(nodeId, { data: metrics, fetchedAt: new Date() });
    updateNodeMetrics(metrics);
  } catch (error) {
    if (controller.signal?.aborted) {
      return;
    }
    state.nodeMetricsAbortController = null;
    destroyNodeMetricsChart();
    setNodeMetricsStatus(
      `Failed to fetch metrics: ${error?.message ?? error}`,
      { isError: true },
    );
  }
}

function updateNodeMetrics(metrics) {
  const array = Array.isArray(metrics)
    ? metrics.slice(Math.max(metrics.length - NODE_METRICS_LIMIT, 0))
    : [];

  if (!array.length) {
    state.nodeMetricsSignature = "";
    destroyNodeMetricsChart();
    setNodeMetricsStatus("No metrics yet");
    return;
  }

  const signature = buildNodeMetricsSignature(array);
  if (signature === state.nodeMetricsSignature && nodeMetricsChart) {
    setNodeMetricsStatus(buildNodeMetricsSummary(array));
    return;
  }

  state.nodeMetricsSignature = signature;

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
      label: "CPU Usage",
      data: cpu,
      borderColor: "rgba(59, 130, 246, 0.85)",
      backgroundColor: "rgba(59, 130, 246, 0.12)",
    });
  }
  if (datasetHasValues(memory)) {
    datasets.push({
      key: "memory",
      label: "Memory Usage",
      data: memory,
      borderColor: "rgba(168, 85, 247, 0.85)",
      backgroundColor: "rgba(168, 85, 247, 0.12)",
    });
  }
  if (datasetHasValues(gpu)) {
    datasets.push({
      key: "gpu",
      label: "GPU Usage",
      data: gpu,
      borderColor: "rgba(34, 197, 94, 0.85)",
      backgroundColor: "rgba(34, 197, 94, 0.12)",
    });
  }
  if (datasetHasValues(gpuMemory)) {
    datasets.push({
      key: "gpu-memory",
      label: "GPU Memory Usage",
      data: gpuMemory,
      borderColor: "rgba(248, 113, 113, 0.85)",
      backgroundColor: "rgba(248, 113, 113, 0.12)",
    });
  }

  if (!datasets.length) {
    destroyNodeMetricsChart();
    setNodeMetricsStatus("Metrics recorded but values unavailable");
    return;
  }

  const shouldRecreate =
    !nodeMetricsChart ||
    nodeMetricsChart.data.datasets.length !== datasets.length ||
    datasets.some((dataset, index) => nodeMetricsChart.data.datasets[index]?.label !== dataset.label);

  if (shouldRecreate) {
    destroyNodeMetricsChart();
    nodeMetricsChart = new Chart(canvas, {
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
    nodeMetricsChart.data.labels = labels;
    datasets.forEach((dataset, index) => {
      nodeMetricsChart.data.datasets[index].data = dataset.data;
      nodeMetricsChart.data.datasets[index].label = dataset.label;
    });
    nodeMetricsChart.update("none");
  }

  setNodeMetricsStatus(buildNodeMetricsSummary(array));
}

function destroyNodeMetricsChart() {
  if (nodeMetricsChart) {
    nodeMetricsChart.destroy();
    nodeMetricsChart = null;
  }
}

function setNodeMetricsStatus(message, { isError = false } = {}) {
  if (!modalRefs.metricsStatus) return;
  modalRefs.metricsStatus.textContent = message;
  modalRefs.metricsStatus.classList.toggle("is-error", isError);
}

function datasetHasValues(values) {
  return values.some((value) => typeof value === "number" && !Number.isNaN(value));
}

function buildNodeMetricsSummary(metrics) {
  const latest = metrics[metrics.length - 1];
  const latestTime = formatMetricLabel(new Date(latest.timestamp));
  const parts = [
    `CPU ${formatPercentage(latest.cpu_usage)}`,
    `Memory ${formatPercentage(latest.memory_usage)}`,
    `GPU ${formatPercentage(latest.gpu_usage)}`,
    `GPU Mem ${formatPercentage(latest.gpu_memory_usage)}`,
  ];
  return `Points: ${metrics.length} / Latest ${latestTime} | ${parts.join(" / ")}`;
}

function buildNodeMetricsSignature(metrics) {
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

async function saveNodeSettings(nodeId) {
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
    const response = await fetch(`/api/nodes/${nodeId}/settings`, {
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
    console.error("Failed to save node settings:", error);
    showError(`Failed to save settings: ${error.message}`);
    throw error;
  }
}

async function deleteNode(nodeId) {
  try {
    const response = await fetch(`/api/nodes/${nodeId}`, {
      method: "DELETE",
      headers: {
        Accept: "application/json",
      },
    });

    if (!response.ok) {
      throw new Error(`${response.status} ${response.statusText}`);
    }
  } catch (error) {
    showError(`Failed to delete node: ${error.message}`);
    throw error;
  }
}

async function disconnectNode(nodeId) {
  try {
    const response = await fetch(`/api/nodes/${nodeId}/disconnect`, {
      method: "POST",
      headers: {
        Accept: "application/json",
      },
    });

    if (!response.ok) {
      throw new Error(`${response.status} ${response.statusText}`);
    }
  } catch (error) {
    showError(`Failed to force disconnect: ${error.message}`);
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
    "runtime_version",
    "status",
    "cpu_usage",
    "memory_usage",
    "gpu_usage",
    "gpu_memory_usage",
    "registered_at",
    "last_seen",
    "tags",
  ];

  const rows = data.map((node) => {
    return [
      node.id,
      getDisplayName(node),
      node.machine_name ?? "",
      node.ip_address ?? "",
      node.runtime_version ?? "",
      node.status ?? "",
      node.cpu_usage ?? "",
      node.memory_usage ?? "",
      node.gpu_usage ?? "",
      node.gpu_memory_usage ?? "",
      node.registered_at ?? "",
      node.last_seen ?? "",
      Array.isArray(node.tags) ? node.tags.join("|") : "",
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
    loading: "Connection: Updating...",
    updating: "Connection: Updating...",
    online: "Connection: OK",
    offline: "Connection: Disconnected",
  };

  pill.textContent = labelMap[mode] ?? "Connection: -";

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
      ? ` / Server: ${formatDate(serverDate)}`
      : "";
  label.textContent = `Last updated: ${clientText}${serverText}`;
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
    return `${days}d ${hours}h`;
  }
  if (hours > 0) {
    return `${hours}h ${minutes}m`;
  }
  if (minutes > 0) {
    return `${minutes}m`;
  }
  return `${abs}s`;
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

function formatReadyProgress(ready) {
  if (!ready || !Array.isArray(ready) || ready.length !== 2) return "";
  const [done, total] = ready;
  if (typeof done !== "number" || typeof total !== "number" || total === 0) return "";
  return `(ready ${done}/${total})`;
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

  // ブラウザのローカルタイムゾーンで表示し、タイムゾーン略称を明示する
  const { timeZone } = Intl.DateTimeFormat().resolvedOptions();
  return new Intl.DateTimeFormat("ja-JP", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
    timeZone,
    timeZoneName: "short",
  }).format(date);
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

// ========================================
// Request History (T029-T032)
// ========================================

let requestHistoryCache = [];
let currentHistoryPage = 1;
let historyPerPage = 50;

async function fetchRequestHistory() {
  try {
    const response = await fetch("/api/dashboard/request-responses");
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }
    const data = await response.json();
    requestHistoryCache = data.records || [];
    renderRequestHistory();
  } catch (error) {
    console.error("Failed to fetch request history:", error);
    const tbody = document.getElementById("request-history-tbody");
    if (tbody) {
      tbody.innerHTML = `<tr><td colspan="8" class="empty-message">Failed to load history</td></tr>`;
    }
  }
}

function renderRequestHistory() {
  const tbody = document.getElementById("request-history-tbody");
  if (!tbody) return;

  const filterModel = document.getElementById("filter-history-model")?.value.toLowerCase() || "";

  let filtered = requestHistoryCache;
  if (filterModel) {
    filtered = filtered.filter(record =>
      record.model?.toLowerCase().includes(filterModel)
    );
  }

  // 新しい順に並べ替え（降順）
  filtered = filtered.sort((a, b) => new Date(b.timestamp) - new Date(a.timestamp));

  if (filtered.length === 0) {
    tbody.innerHTML = `<tr><td colspan="8" class="empty-message">No history</td></tr>`;
    updateHistoryPagination(0, 0);
    return;
  }

  // ページネーション計算
  const totalItems = filtered.length;
  const totalPages = Math.ceil(totalItems / historyPerPage);
  const startIndex = (currentHistoryPage - 1) * historyPerPage;
  const endIndex = Math.min(startIndex + historyPerPage, totalItems);
  const pageItems = filtered.slice(startIndex, endIndex);

  const rows = pageItems.map(record => {
    const timestamp = new Date(record.timestamp);
    const statusClass = record.status.type === "success" ? "status-success" : "status-error";
    const statusText = record.status.type === "success"
      ? "Success"
      : `Error: ${escapeHtml(record.status.message || "Unknown")}`;
    const clientIp = record.client_ip || "-";

    return `
      <tr data-record-id="${escapeHtml(record.id)}">
        <td>${formatTimestamp(timestamp)}</td>
        <td>${escapeHtml(record.request_type)}</td>
        <td>${escapeHtml(record.model)}</td>
        <td title="${escapeHtml(record.agent_ip)}">${escapeHtml(record.agent_machine_name)}</td>
        <td>${escapeHtml(clientIp)}</td>
        <td>${escapeHtml(record.duration_ms)}ms</td>
        <td><span class="${statusClass}">${statusText}</span></td>
        <td><button class="btn btn-sm view-request-detail" data-id="${escapeHtml(record.id)}">Details</button></td>
      </tr>
    `;
  }).join("");

  tbody.innerHTML = rows;

  // 詳細ボタンにイベントリスナーを追加
  tbody.querySelectorAll(".view-request-detail").forEach(btn => {
    btn.addEventListener("click", () => {
      const id = btn.dataset.id;
      showRequestDetail(id);
    });
  });

  // ページネーション情報を更新
  updateHistoryPagination(currentHistoryPage, totalPages);
}

function updateHistoryPagination(currentPage, totalPages) {
  const pageInfo = document.getElementById("history-page-info");
  const prevBtn = document.getElementById("history-page-prev");
  const nextBtn = document.getElementById("history-page-next");

  if (pageInfo) {
    pageInfo.textContent = totalPages > 0 ? `${currentPage} / ${totalPages}` : "- / -";
  }

  if (prevBtn) {
    prevBtn.disabled = currentPage <= 1;
  }

  if (nextBtn) {
    nextBtn.disabled = currentPage >= totalPages || totalPages === 0;
  }
}

async function showRequestDetail(id) {
  try {
    const response = await fetch(`/api/dashboard/request-responses/${id}`);
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }
    const record = await response.json();

    document.getElementById("request-detail-id").textContent = record.id;
    document.getElementById("request-detail-timestamp").textContent = formatTimestamp(new Date(record.timestamp));
    document.getElementById("request-detail-type").textContent = record.request_type;
    document.getElementById("request-detail-model").textContent = record.model;
    document.getElementById("request-detail-node").textContent = `${record.node_machine_name} (${record.node_ip})`;
    document.getElementById("request-detail-client-ip").textContent = record.client_ip || "Not available";
    document.getElementById("request-detail-duration").textContent = `${record.duration_ms}ms`;

    const statusText = record.status.type === "success"
      ? "Success"
      : `Error: ${record.status.message || "Unknown"}`;
    document.getElementById("request-detail-status").textContent = statusText;

    document.getElementById("request-detail-request-body").textContent =
      JSON.stringify(record.request_body, null, 2);

    document.getElementById("request-detail-response-body").textContent =
      record.response_body ? JSON.stringify(record.response_body, null, 2) : "(No response)";

    // モーダル表示
    const modal = document.getElementById("request-modal");
    if (modal) {
      modal.classList.remove("hidden");
    }
  } catch (error) {
    console.error("Failed to fetch request detail:", error);
    alert("Failed to load request details");
  }
}

function exportHistoryCSV() {
  window.location.href = "/api/dashboard/request-responses/export";
}

// リクエスト履歴の初期化
document.addEventListener("DOMContentLoaded", () => {
  // CSVエクスポートボタン
  const exportBtn = document.getElementById("export-history-csv");
  if (exportBtn) {
    exportBtn.addEventListener("click", exportHistoryCSV);
  }

  // モデルフィルタ
  const filterModel = document.getElementById("filter-history-model");
  if (filterModel) {
    filterModel.addEventListener("input", () => {
      currentHistoryPage = 1; // フィルタ変更時は1ページ目にリセット
      renderRequestHistory();
    });
  }

  // 表示件数切り替え
  const perPageSelect = document.getElementById("history-per-page");
  if (perPageSelect) {
    perPageSelect.addEventListener("change", (e) => {
      historyPerPage = parseInt(e.target.value, 10);
      currentHistoryPage = 1; // 表示件数変更時は1ページ目にリセット
      renderRequestHistory();
    });
  }

  // ページネーションボタン
  const prevBtn = document.getElementById("history-page-prev");
  const nextBtn = document.getElementById("history-page-next");

  if (prevBtn) {
    prevBtn.addEventListener("click", () => {
      if (currentHistoryPage > 1) {
        currentHistoryPage--;
        renderRequestHistory();
      }
    });
  }

  if (nextBtn) {
    nextBtn.addEventListener("click", () => {
      currentHistoryPage++;
      renderRequestHistory();
    });
  }

  // モーダルクローズボタン
  const requestModalClose = document.getElementById("request-modal-close");
  const requestModalOk = document.getElementById("request-modal-ok");
  const requestModal = document.getElementById("request-modal");

  if (requestModalClose && requestModal) {
    requestModalClose.addEventListener("click", () => {
      requestModal.classList.add("hidden");
    });
  }

  if (requestModalOk && requestModal) {
    requestModalOk.addEventListener("click", () => {
      requestModal.classList.add("hidden");
    });
  }

  // 初回読み込み
  fetchRequestHistory();

  // 定期更新（30秒ごと）
  setInterval(fetchRequestHistory, 30000);
});

// ========== ログビューア ==========

function initLogControls() {
  if (logRefs.coordinatorRefresh) {
    logRefs.coordinatorRefresh.addEventListener("click", () => {
      fetchCoordinatorLogs({ skipIfFetched: false });
    });
  }

  if (logRefs.nodeRefresh) {
    logRefs.nodeRefresh.addEventListener("click", () => {
      state.logs.nodeFetched = false;
      fetchNodeLogs({ skipIfFetched: false });
    });
  }

  if (logRefs.nodeSelect) {
    logRefs.nodeSelect.addEventListener("change", (event) => {
      const nextId = event.target.value || null;
      state.logs.selectedNodeId = nextId;
      state.logs.nodeFetched = false;
      if (nextId) {
        fetchNodeLogs({ skipIfFetched: false });
      } else {
        state.logs.node = [];
        state.logs.nodePath = null;
        state.logs.nodeError = null;
        renderNodeLogs();
      }
    });
  }
}

function initModalLogControls() {
  if (modalRefs.logRefresh) {
    modalRefs.logRefresh.addEventListener("click", () => {
      if (state.currentNodeId) {
        loadModalNodeLogs(state.currentNodeId, { force: true });
      }
    });
  }
}

function maybeRefreshLogs(force = false) {
  fetchCoordinatorLogs({ skipIfFetched: !force });
  if (state.logs.selectedNodeId) {
    fetchNodeLogs({ skipIfFetched: !force });
  } else {
    renderNodeLogs();
  }
}

async function fetchCoordinatorLogs({ skipIfFetched = false } = {}) {
  if (skipIfFetched && state.logs.coordinatorFetched) {
    return;
  }

  state.logs.loadingCoordinator = true;
  state.logs.coordinatorError = null;
  renderCoordinatorLogs();

  try {
    const data = await fetchJson(`/api/dashboard/logs/coordinator?limit=${LOG_ENTRY_LIMIT}`);
    state.logs.coordinator = Array.isArray(data.entries) ? data.entries : [];
    state.logs.coordinatorPath = typeof data.path === "string" ? data.path : null;
    state.logs.coordinatorFetched = true;
  } catch (error) {
    state.logs.coordinatorError = `Failed to fetch logs: ${error?.message ?? error}`;
  } finally {
    state.logs.loadingCoordinator = false;
    renderCoordinatorLogs();
  }
}

async function fetchNodeLogs({ skipIfFetched = false } = {}) {
  if (!state.logs.selectedNodeId) {
    state.logs.node = [];
    state.logs.nodeFetched = false;
    state.logs.nodeError = null;
    state.logs.nodePath = null;
    renderNodeLogs();
    return;
  }

  if (skipIfFetched && state.logs.nodeFetched) {
    return;
  }

  state.logs.loadingNode = true;
  state.logs.nodeError = null;
  renderNodeLogs();

  try {
    const nodeId = encodeURIComponent(state.logs.selectedNodeId);
    const data = await fetchJson(`/api/dashboard/logs/nodes/${nodeId}?limit=${LOG_ENTRY_LIMIT}`);
    state.logs.node = Array.isArray(data.entries) ? data.entries : [];
    state.logs.nodePath = typeof data.path === "string" ? data.path : null;
    state.logs.nodeFetched = true;
  } catch (error) {
    state.logs.nodeError = `Failed to fetch logs: ${error?.message ?? error}`;
    state.logs.nodeFetched = false;
  } finally {
    state.logs.loadingNode = false;
    renderNodeLogs();
  }
}

function renderCoordinatorLogs() {
  renderLogViewer(logRefs.coordinatorList, {
    entries: state.logs.coordinator,
    loading: state.logs.loadingCoordinator,
    error: state.logs.coordinatorError,
    emptyMessage: "No logs yet",
  });

  if (logRefs.coordinatorPath) {
    logRefs.coordinatorPath.textContent = state.logs.coordinatorPath
      ? `Path: ${state.logs.coordinatorPath}`
      : "";
  }

  if (logRefs.coordinatorStatus) {
    if (state.logs.loadingCoordinator) {
      logRefs.coordinatorStatus.textContent = "Loading...";
    } else if (state.logs.coordinatorError) {
      logRefs.coordinatorStatus.textContent = "An error occurred";
    } else {
      logRefs.coordinatorStatus.textContent = `Showing latest ${state.logs.coordinator.length} entries`;
    }
  }
}

function renderNodeLogs() {
  const hasNodes = state.nodes.length > 0;
  const emptyMessage = state.logs.selectedNodeId
    ? "No logs yet"
    : hasNodes
      ? "Please select a node"
      : "No nodes registered";
  const errorMessage = state.logs.selectedNodeId ? state.logs.nodeError : null;

  renderLogViewer(logRefs.nodeList, {
    entries: state.logs.node,
    loading: state.logs.loadingNode,
    error: errorMessage,
    emptyMessage,
  });

  if (logRefs.nodePath) {
    logRefs.nodePath.textContent = state.logs.nodePath
      ? `Path: ${state.logs.nodePath}`
      : "";
  }

  if (logRefs.nodeStatus) {
    if (state.logs.loadingNode) {
      logRefs.nodeStatus.textContent = "Loading...";
    } else if (errorMessage) {
      logRefs.nodeStatus.textContent = errorMessage;
    } else if (state.logs.selectedNodeId) {
      logRefs.nodeStatus.textContent = `Showing latest ${state.logs.node.length} entries`;
    } else {
      logRefs.nodeStatus.textContent = emptyMessage;
    }
  }
}

function renderLogViewer(target, { entries, loading, error, emptyMessage }) {
  if (!target) return;

  if (loading) {
    target.innerHTML = '<div class="log-placeholder">Loading...</div>';
    return;
  }

  if (error) {
    target.innerHTML = `<div class="log-placeholder log-placeholder--error">${escapeHtml(
      error,
    )}</div>`;
    return;
  }

  if (!entries || !entries.length) {
    target.innerHTML = `<div class="log-placeholder">${escapeHtml(emptyMessage)}</div>`;
    return;
  }

  const lines = entries
    .slice()
    .reverse()
    .map(renderLogLine)
    .join("");
  target.innerHTML = lines;
}

function renderLogLine(entry) {
  const rawLevel = typeof entry?.level === "string" ? entry.level : "info";
  const level = rawLevel.toLowerCase();
  const levelClass = level.replace(/[^a-z]/g, "") || "info";
  const timestamp = formatLogTimestamp(entry?.timestamp);
  const targetLabel = entry?.target ? String(entry.target) : "-";
  const context = formatLogFields(entry?.fields);
  const hasMessage = typeof entry?.message === "string" && entry.message.length > 0;
  const message = hasMessage ? entry.message : context || "-";
  const fileInfo = entry?.file
    ? `${entry.file}${entry.line != null ? `:${entry.line}` : ""}`
    : "";

  return `
    <div class="log-line log-line--${levelClass}">
      <span class="log-line__time">${escapeHtml(timestamp)}</span>
      <span class="log-line__level">${escapeHtml(level.toUpperCase())}</span>
      <div class="log-line__body">
        <div class="log-line__message">${escapeHtml(message)}</div>
        <div class="log-line__meta">
          <span>${escapeHtml(targetLabel)}</span>
          ${fileInfo ? `<span>${escapeHtml(fileInfo)}</span>` : ""}
          ${context && hasMessage ? `<span>${escapeHtml(context)}</span>` : ""}
        </div>
      </div>
    </div>
  `;
}

function formatLogTimestamp(value) {
  if (!value) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  const time = date.toLocaleTimeString("ja-JP", {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
  const millis = String(date.getMilliseconds()).padStart(3, "0");
  return `${time}.${millis}`;
}

function formatLogFields(fields) {
  if (!fields || typeof fields !== "object") return "";
  const entries = Object.entries(fields)
    .filter(([, value]) => value !== undefined)
    .map(([key, value]) => `${key}=${summarizeFieldValue(value)}`);
  return entries.length ? entries.join(" · ") : "";
}

function summarizeFieldValue(value) {
  if (value == null) return "null";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function renderLogsNodeOptions() {
  if (!logRefs.nodeSelect) return;

  const select = logRefs.nodeSelect;
  const nodes = Array.isArray(state.nodes) ? state.nodes : [];
  const previousSelection = state.logs.selectedNodeId;
  const hasNodes = nodes.length > 0;

  const options = nodes
    .map((node) => {
      const label =
        node.machine_name && node.machine_name.trim().length
          ? node.machine_name
          : node.id.slice(0, 8);
      const statusLabel = node.status === "online" ? "Online" : "Offline";
      return `<option value="${escapeHtml(node.id)}">${escapeHtml(label)} (${statusLabel})</option>`;
    })
    .join("");

  select.innerHTML = `<option value="">Select a node</option>${options}`;

  if (previousSelection && nodes.some((node) => node.id === previousSelection)) {
    select.value = previousSelection;
  } else if (hasNodes) {
    const fallback =
      nodes.find((node) => node.status === "online") ?? nodes[0];
    state.logs.selectedNodeId = fallback.id;
    state.logs.nodeFetched = false;
    select.value = fallback.id;
  } else {
    state.logs.selectedNodeId = null;
    state.logs.nodeFetched = false;
    select.value = "";
  }

  const disabled = !hasNodes;
  select.disabled = disabled;
  if (logRefs.nodeRefresh) {
    logRefs.nodeRefresh.disabled = disabled;
  }

  if (!state.logs.selectedNodeId) {
    state.logs.node = [];
    state.logs.nodePath = null;
    state.logs.nodeError = null;
    renderNodeLogs();
  }
}

function resetModalNodeLogs() {
  state.modalLog.entries = [];
  state.modalLog.path = null;
  state.modalLog.error = null;
  state.modalLog.loading = false;
  state.modalLog.fetchedNodeId = null;
  renderModalNodeLogs();
}

async function loadModalNodeLogs(nodeId, { force = false } = {}) {
  if (!nodeId || !modalRefs.logViewer) return;
  if (!force && state.modalLog.fetchedNodeId === nodeId && !state.modalLog.error) {
    return;
  }

  state.modalLog.loading = true;
  state.modalLog.error = null;
  state.modalLog.fetchedNodeId = nodeId;
  renderModalNodeLogs();

  try {
    const payload = await fetchJson(
      `/api/dashboard/logs/nodes/${nodeId}?limit=${MODAL_LOG_ENTRY_LIMIT}`,
    );
    state.modalLog.entries = Array.isArray(payload.entries) ? payload.entries : [];
    state.modalLog.path = typeof payload.path === "string" ? payload.path : null;
    state.modalLog.error = null;
  } catch (error) {
    state.modalLog.entries = [];
    state.modalLog.error = `Failed to fetch logs: ${error?.message ?? error}`;
  } finally {
    state.modalLog.loading = false;
    renderModalNodeLogs();
  }
}

function renderModalNodeLogs() {
  if (!modalRefs.logViewer) return;
  const emptyMessage = state.currentNodeId
    ? "No logs yet"
    : "No node selected";

  renderLogViewer(modalRefs.logViewer, {
    entries: state.modalLog.entries,
    loading: state.modalLog.loading,
    error: state.modalLog.error,
    emptyMessage,
  });

  if (modalRefs.logStatus) {
    if (state.modalLog.loading) {
      modalRefs.logStatus.textContent = "Loading logs...";
    } else if (state.modalLog.error) {
      modalRefs.logStatus.textContent = state.modalLog.error;
    } else if (state.modalLog.entries.length) {
      modalRefs.logStatus.textContent = `Showing latest ${state.modalLog.entries.length} entries`;
    } else {
      modalRefs.logStatus.textContent = emptyMessage;
    }
  }

  if (modalRefs.logPath) {
    modalRefs.logPath.textContent = state.modalLog.path ? `Path: ${state.modalLog.path}` : "";
  }

  if (modalRefs.logRefresh) {
    modalRefs.logRefresh.disabled = !state.currentNodeId || state.modalLog.loading;
  }
}

// ========== タブ管理 ==========

/**
 * タブ切り替え処理
 */
function switchTab(tabName) {
  // タブUIは廃止。全パネルを常時表示し、currentTab はログ自動更新の判定にだけ使う。
  document.querySelectorAll('.tab-panel').forEach((panel) => {
    panel.classList.add('tab-panel--active');
    panel.removeAttribute('aria-hidden');
  });

  state.currentTab = tabName;
}

/**
 * タブ切り替えイベントリスナーを登録
 */
function initTabs() {
  // 旧タブパネルは全て表示状態にしておく
  switchTab('all');
}
