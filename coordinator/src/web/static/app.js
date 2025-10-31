const REFRESH_INTERVAL_MS = 5000;

const state = {
  agents: [],
  stats: null,
  timerId: null,
};

document.addEventListener("DOMContentLoaded", () => {
  const refreshButton = document.getElementById("refresh-button");
  const filterCheckbox = document.getElementById("filter-offline");

  refreshButton.addEventListener("click", () => refreshData({ manual: true }));
  filterCheckbox.addEventListener("change", () => renderAgents());

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
    const [agents, stats] = await Promise.all([
      fetchJson("/api/dashboard/agents"),
      fetchJson("/api/dashboard/stats"),
    ]);

    state.agents = agents;
    state.stats = stats;

    renderStats();
    renderAgents();
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
  const hideOffline = document.getElementById("filter-offline").checked;

  tbody.innerHTML = "";

  if (!state.agents.length) {
    const placeholder = document.createElement("tr");
    placeholder.className = "empty-row";
    placeholder.innerHTML = `<td colspan="10">エージェントはまだ登録されていません</td>`;
    tbody.appendChild(placeholder);
    return;
  }

  const fragment = document.createDocumentFragment();
  state.agents
    .filter((agent) => !(hideOffline && agent.status === "offline"))
    .forEach((agent) => fragment.appendChild(buildAgentRow(agent)));

  if (!fragment.childNodes.length) {
    const placeholder = document.createElement("tr");
    placeholder.className = "empty-row";
    placeholder.innerHTML = `<td colspan="10">表示対象のエージェントはありません</td>`;
    tbody.appendChild(placeholder);
    return;
  }

  tbody.appendChild(fragment);
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
  `;

  return tr;
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
