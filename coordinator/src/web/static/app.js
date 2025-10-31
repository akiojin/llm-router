const REFRESH_INTERVAL_MS = 5000;

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
  timerId: null,
};

let requestsChart = null;
const modalRefs = {
  modal: null,
  close: null,
  ok: null,
  machineName: null,
  ipAddress: null,
  ollamaVersion: null,
  uptime: null,
  status: null,
  lastSeen: null,
  totalRequests: null,
  averageResponse: null,
  customName: null,
  tags: null,
  notes: null,
};

document.addEventListener("DOMContentLoaded", () => {
  const refreshButton = document.getElementById("refresh-button");
  const statusSelect = document.getElementById("filter-status");
  const queryInput = document.getElementById("filter-query");
  const sortableHeaders = document.querySelectorAll("th[data-sort]");
  const selectAllCheckbox = document.getElementById("select-all");
  const modal = document.getElementById("agent-modal");
  const modalClose = document.getElementById("agent-modal-close");
  const modalOk = document.getElementById("agent-modal-ok");
  const tbody = document.getElementById("agents-body");

  Object.assign(modalRefs, {
    modal,
    close: modalClose,
    ok: modalOk,
    machineName: document.getElementById("detail-machine-name"),
    ipAddress: document.getElementById("detail-ip-address"),
    ollamaVersion: document.getElementById("detail-ollama-version"),
    uptime: document.getElementById("detail-uptime"),
    status: document.getElementById("detail-status"),
    lastSeen: document.getElementById("detail-last-seen"),
    totalRequests: document.getElementById("detail-total-requests"),
    averageResponse: document.getElementById("detail-average-response"),
    customName: document.getElementById("detail-custom-name"),
    tags: document.getElementById("detail-tags"),
    notes: document.getElementById("detail-notes"),
  });

  refreshButton.addEventListener("click", () => refreshData({ manual: true }));
  statusSelect.addEventListener("change", (event) => {
    state.filterStatus = event.target.value;
    renderAgents();
  });
  let queryDebounce = null;
  queryInput.addEventListener("input", (event) => {
    const value = event.target.value ?? "";
    window.clearTimeout(queryDebounce);
    queryDebounce = window.setTimeout(() => {
    state.filterQuery = value.trim().toLowerCase();
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

  try {
    const [agents, stats, history] = await Promise.all([
      fetchJson("/api/dashboard/agents"),
      fetchJson("/api/dashboard/stats"),
      fetchJson("/api/dashboard/request-history"),
    ]);

    state.agents = agents;
    state.stats = stats;
    state.history = history;

    renderStats();
    renderAgents();
    renderHistory();
    hideError();
    setConnectionStatus("online");
    updateLastRefreshed(new Date());
  } catch (error) {
    console.error("Dashboard refresh failed:", error);
    showError(`ダッシュボードデータの取得に失敗しました: ${error.message}`);
    setConnectionStatus("offline");
  }
}

async function fetchJson(url) {
  const response = await fetch(url, {
    method: "GET",
    cache: "no-store",
    headers: {
      "Accept": "application/json",
    },
  });

  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
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
    "last-metrics-updated-at": formatTimestamp(state.stats.last_metrics_updated_at),
    "last-registered-at": formatTimestamp(state.stats.last_registered_at),
    "last-seen-at": formatTimestamp(state.stats.last_seen_at),
  };

  Object.entries(statsMap).forEach(([key, value]) => {
    const target = document.querySelector(`[data-stat="${key}"]`);
    if (target) {
      target.textContent = value ?? "-";
    }
  });
}

function renderAgents() {
  const tbody = document.getElementById("agents-body");

  tbody.innerHTML = "";

  if (!state.agents.length) {
    const placeholder = document.createElement("tr");
    placeholder.className = "empty-row";
    placeholder.innerHTML = `<td colspan="11">エージェントはまだ登録されていません</td>`;
    tbody.appendChild(placeholder);
    return;
  }

  const filtered = state.agents.filter((agent) =>
    filterAgent(agent, state.filterStatus, state.filterQuery),
  );

  if (!filtered.length) {
    const placeholder = document.createElement("tr");
    placeholder.className = "empty-row";
    placeholder.innerHTML = `<td colspan="11">条件に一致するエージェントはありません</td>`;
    tbody.appendChild(placeholder);
    return;
  }

  const sorted = sortAgents(filtered, state.sortKey, state.sortOrder);

  const fragment = document.createDocumentFragment();
  sorted.forEach((agent) => fragment.appendChild(buildAgentRow(agent)));

  tbody.appendChild(fragment);
}

function renderHistory() {
  const canvas = document.getElementById("requests-chart");
  if (!canvas || typeof Chart === "undefined") {
    return;
  }

  if (!Array.isArray(state.history) || !state.history.length) {
    const labels = buildHistoryLabels([]);
    const zeroes = new Array(labels.length).fill(0);
    updateHistoryChart(canvas, labels, zeroes, zeroes);
    return;
  }

  const labels = buildHistoryLabels(state.history);
  const success = state.history.map((point) => point.success ?? 0);
  const failures = state.history.map((point) => point.error ?? 0);
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
              color: "var(--text-subtle)",
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
              color: "var(--text-subtle)",
              maxRotation: 0,
            },
            grid: {
              color: "rgba(148, 163, 184, 0.08)",
            },
          },
          y: {
            beginAtZero: true,
            ticks: {
              color: "var(--text-subtle)",
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

function buildAgentRow(agent) {
  const tr = document.createElement("tr");
  tr.dataset.agentId = agent.id;
  if (agent.status === "offline") {
    tr.classList.add("agent-offline");
  }

  const statusLabel =
    agent.status === "online"
      ? '<span class="badge badge--online">Online</span>'
      : '<span class="badge badge--offline">Offline</span>';

  const metricsBadge = agent.metrics_stale
    ? '<span class="badge badge--stale">STALE</span>'
    : "";
  const metricsTimestamp = formatTimestamp(agent.metrics_last_updated_at);
  const metricsDetail = metricsBadge ? `${metricsBadge} ${metricsTimestamp}` : metricsTimestamp;

  tr.innerHTML = `
    <td>
      <input
        type="checkbox"
        data-agent-id="${agent.id}"
        ${state.selection.has(agent.id) ? "checked" : ""}
        aria-label="${escapeHtml(agent.machine_name)} を選択"
      />
    </td>
    <td>
      <div class="cell-title">${escapeHtml(agent.machine_name)}</div>
      <div class="cell-sub">${escapeHtml(agent.ollama_version)}</div>
    </td>
    <td>
      <div class="cell-title">${escapeHtml(agent.ip_address)}</div>
      <div class="cell-sub">Port ${Number.isFinite(agent.ollama_port) ? escapeHtml(agent.ollama_port) : "-"}</div>
    </td>
    <td>${statusLabel}</td>
    <td>${formatDuration(agent.uptime_seconds)}</td>
    <td>${formatPercentage(agent.cpu_usage)}</td>
    <td>${formatPercentage(agent.memory_usage)}</td>
    <td>${agent.active_requests}</td>
    <td>
      <div class="cell-title">${agent.total_requests}</div>
      <div class="cell-sub">
        成功 ${agent.successful_requests} / 失敗 ${agent.failed_requests}
      </div>
    </td>
    <td>${formatAverage(agent.average_response_time_ms)}</td>
    <td>
      <div class="cell-title">${formatTimestamp(agent.last_seen)}</div>
      <div class="cell-sub">${metricsDetail}</div>
    </td>
    <td>
      <button type="button" data-agent-id="${agent.id}">詳細</button>
    </td>
  `;

  return tr;
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
  return machine.includes(query) || ip.includes(query);
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
      return localeCompare(a.machine_name, b.machine_name);
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

function openAgentModal(agent) {
  if (!modalRefs.modal) return;
  state.lastFocused = document.activeElement;
  state.selection = new Set([agent.id]);

  modalRefs.machineName.textContent = agent.machine_name ?? "-";
  modalRefs.ipAddress.textContent = agent.ip_address ?? "-";
  modalRefs.ollamaVersion.textContent = agent.ollama_version ?? "-";
  modalRefs.uptime.textContent = formatDuration(agent.uptime_seconds);
  modalRefs.status.textContent = agent.status === "online" ? "オンライン" : "オフライン";
  modalRefs.lastSeen.textContent = formatTimestamp(agent.last_seen);
  modalRefs.totalRequests.textContent = agent.total_requests ?? 0;
  modalRefs.averageResponse.textContent = formatAverage(agent.average_response_time_ms);
  modalRefs.customName.value = agent.custom_name ?? "";
  modalRefs.tags.value = Array.isArray(agent.tags) ? agent.tags.join(", ") : "";
  modalRefs.notes.value = agent.notes ?? "";

  modalRefs.modal.classList.remove("hidden");
  modalRefs.modal.setAttribute("tabindex", "-1");
  window.requestAnimationFrame(() => modalRefs.close.focus());
}

function closeAgentModal() {
  if (!modalRefs.modal) return;
  modalRefs.modal.classList.add("hidden");
  if (state.lastFocused && typeof state.lastFocused.focus === "function") {
    state.lastFocused.focus();
  }
}

async function saveAgentSettings(agent) {
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
    const response = await fetch(`/api/agents/${agent.id}/settings`, {
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
  } catch (error) {
    console.error("Failed to save agent settings:", error);
    showError(`設定の保存に失敗しました: ${error.message}`);
    throw error;
  }
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

function updateLastRefreshed(date) {
  const label = document.getElementById("last-refreshed");
  if (!label) return;
  label.textContent = `最終更新: ${formatDate(date)}`;
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
  state.selectAll =
    filtered.length > 0 && filtered.every((agent) => state.selection.has(agent.id));
  document.getElementById("select-all").checked = state.selectAll;
