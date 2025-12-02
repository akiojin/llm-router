/**
 * Model Management UI
 *
 * Manages model distribution, progress tracking, and model info display
 */

// ========== Constants ==========
const MODEL_PRESETS = {
  'gpt-oss:20b': {
    gpuHint: 'Recommended GPU: 16GB+',
    badge: 'GPT-OSS 20B',
    usage: 'High precision, long context, general/code',
    sizeGb: 14.5,
  },
  'gpt-oss-safeguard:20b': {
    gpuHint: 'Recommended GPU: 16GB+',
    badge: 'Safety 20B',
    usage: 'Moderation and safety evaluation',
    sizeGb: 14,
  },
  'gpt-oss:120b': {
    gpuHint: 'Recommended GPU: 80GB+',
    badge: 'GPT-OSS 120B',
    usage: 'Highest precision, requires large GPU',
    sizeGb: 65,
  },
  'qwen3-coder:30b': {
    gpuHint: 'Recommended GPU: 24GB+',
    badge: 'Qwen3 Coder 30B',
    usage: 'Latest generation code generation',
    sizeGb: 17,
  },
};

// ========== State Management ==========
let availableModels = [];
let registeredModels = [];
let availableModelsMeta = { source: null };
let selectedModel = null;
let downloadTasks = new Map(); // task_id -> task info
let progressPollingInterval = null;
let modelFilterQuery = '';
let cachedAgents = [];
let loadedModels = [];
let manualPanelOpen = false;

// ========== DOM Elements ==========
const elements = {
  availableModelsList: () => document.getElementById('available-models-list'),
  hfModelsList: () => document.getElementById('hf-models-list'),
  registeredModelsList: () => document.getElementById('registered-models-list'),
  agentsForDistribution: () => document.getElementById('agents-for-distribution'),
  distributeButton: () => document.getElementById('distribute-model-button'),
  selectAllButton: () => document.getElementById('select-all-agents'),
  deselectAllButton: () => document.getElementById('deselect-all-agents'),
  downloadTasksList: () => document.getElementById('download-tasks-list'),
  loadedModelsList: () => document.getElementById('loaded-models-list'),
  manualPanel: () => document.getElementById('manual-distribution-panel'),
  toggleManualBtn: () => document.getElementById('toggle-manual-distribution'),
  modelsCount: () => document.getElementById('models-count'),
  selectedModelNameDisplay: () => document.getElementById('selected-model-name'),
  selectedModelTitle: () => document.getElementById('selected-model-title'),
  selectedModelDescription: () => document.getElementById('selected-model-description'),
  selectedModelSize: () => document.getElementById('selected-model-size'),
  selectedModelGpu: () => document.getElementById('selected-model-gpu'),
  modelsOnlineCount: () => document.getElementById('models-online-count'),
  agentsSelectionSummary: () => document.getElementById('agents-selection-summary'),
  distributeHint: () => document.querySelector('.distribute-hint'),
  modelSearchInput: () => document.getElementById('model-search-input'),
  modelsSourceLabel: () => document.getElementById('models-source-label'),
  selectedModelCard: () => document.getElementById('selected-model-card'),
  hfSearchInput: () => document.getElementById('hf-search'),
  hfRefreshBtn: () => document.getElementById('hf-refresh'),
  registeredRefreshBtn: () => document.getElementById('registered-refresh'),
  hfStatus: () => document.getElementById('hf-models-status'),
  tasksRefreshBtn: () => document.getElementById('download-tasks-refresh'),
};

// ========== API Functions ==========

/**
 * GET /api/models/available - Fetch available models list
 */
async function fetchAvailableModels() {
  try {
    const params = new URLSearchParams();
    params.set('source', 'hf');
    const q = elements.hfSearchInput()?.value;
    if (q) params.set('search', q);
    const response = await fetch(`/api/models/available?${params.toString()}`);
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    }
    const data = await response.json();
    availableModelsMeta.source = data.source ?? null;
    return data.models || [];
  } catch (error) {
    console.error('Failed to fetch available models:', error);
    showError('Failed to fetch available models');
    return [];
  }
}

async function fetchRegisteredModels() {
  try {
    const response = await fetch('/v1/models');
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const data = await response.json();
    const list = Array.isArray(data.data) ? data.data : [];
    return list.map((m) => ({
      name: m.id ?? '',
      description: m.description ?? '',
      display_name: m.id ?? '',
      size_gb: m.size_gb ?? undefined,
      tags: m.tags ?? [],
    }));
  } catch (error) {
    console.error('Failed to fetch registered models:', error);
    showError('Failed to fetch registered models');
    return [];
  }
}

async function registerModel(repo, filename, displayName) {
  const payload = { repo, filename, display_name: displayName };
  const response = await fetch('/api/models/register', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    const txt = await response.text();
    throw new Error(txt || `HTTP ${response.status}`);
  }
  return response.json();
}

async function pullModelFromHf(repo, filename, chatTemplate) {
  const payload = { repo, filename, chat_template: chatTemplate };
  const response = await fetch('/api/models/pull', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    const txt = await response.text();
    throw new Error(txt || `HTTP ${response.status}`);
  }
  return response.json();
}

/**
 * POST /api/models/distribute - Distribute model
 */
async function distributeModel(modelName, target, agentIds = []) {
  try {
    const payload = {
      model_name: modelName,
      target,
      ...(target === 'specific' && { agent_ids: agentIds }),
    };

    const response = await fetch('/api/models/distribute', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    });

    if (!response.ok) {
      const error = await response.json();
      throw new Error(error.error || `HTTP ${response.status}`);
    }

    const data = await response.json();
    return data.task_ids || [];
  } catch (error) {
    console.error('Failed to distribute model:', error);
    showError(`Failed to distribute model: ${error.message}`);
    return [];
  }
}

/**
 * GET /api/agents/{agent_id}/models - Fetch installed models for agent
 */
async function fetchAgentModels(agentId) {
  try {
    const response = await fetch(`/api/agents/${agentId}/models`);
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }
    return await response.json();
  } catch (error) {
    console.error(`Failed to fetch models for agent ${agentId}:`, error);
    return [];
  }
}

/**
 * GET /api/tasks - Fetch all active tasks
 */
async function fetchActiveTasks() {
  try {
    const response = await fetch('/api/tasks');
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }
    return await response.json();
  } catch (error) {
    console.error('Failed to fetch active tasks:', error);
    return [];
  }
}

/**
 * GET /api/tasks/{task_id} - Fetch task progress
 */
async function fetchTaskProgress(taskId) {
  try {
    const response = await fetch(`/api/tasks/${taskId}`);
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }
    return await response.json();
  } catch (error) {
    console.error(`Failed to fetch task ${taskId}:`, error);
    return null;
  }
}

/**
 * GET /api/models/loaded - Fetch loaded models summary across all agents
 */
async function fetchLoadedModels() {
  try {
    const response = await fetch('/api/models/loaded');
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }
    return await response.json();
  } catch (error) {
    console.error('Failed to fetch loaded models:', error);
    return [];
  }
}

// ========== Helpers ==========

function escapeHtml(value) {
  if (value == null) return '';
  return String(value)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function formatModelSize(sizeGb) {
  if (typeof sizeGb !== 'number' || Number.isNaN(sizeGb)) {
    return 'Size unknown';
  }
  return `${sizeGb.toFixed(1)} GB`;
}

function computeSizeGbFromModel(model) {
  if (typeof model?.size_gb === 'number') {
    return model.size_gb;
  }
  if (typeof model?.size === 'number') {
    return model.size / (1024 ** 3);
  }
  // Fallback: estimate from required memory
  const reqGb = computeRequiredMemoryGb(model);
  if (typeof reqGb === 'number') {
    return reqGb;
  }
  const preset = getModelPreset(model?.name);
  if (preset?.sizeGb) {
    return preset.sizeGb;
  }
  return null;
}

function computeRequiredMemoryGb(model) {
  if (typeof model?.required_memory_gb === 'number') {
    return model.required_memory_gb;
  }
  if (typeof model?.required_memory === 'number') {
    return model.required_memory / (1024 ** 3);
  }
  const preset = getModelPreset(model?.name);
  if (preset?.sizeGb) {
    // If no required memory info, use size as fallback
    return preset.sizeGb;
  }
  return null;
}

function getModelPreset(modelName) {
  return MODEL_PRESETS[modelName] ?? null;
}

function getGpuHint(model) {
  const preset = getModelPreset(model.name);
  if (preset?.gpuHint) {
    return preset.gpuHint;
  }
  const requiredGb = computeRequiredMemoryGb(model);
  const sizeGb = computeSizeGbFromModel(model);
  const basis = typeof requiredGb === 'number' ? requiredGb : sizeGb;
  if (typeof basis === 'number') {
    if (basis >= 12) return 'Recommended GPU: 16GB+';
    if (basis >= 8) return 'Recommended GPU: 8-16GB';
    if (basis >= 4.5) return 'Recommended GPU: 4.5-8GB';
    return 'Recommended GPU: <4.5GB';
  }
  return 'Recommended GPU: Info not available';
}

function getPresetUsage(model) {
  const preset = getModelPreset(model.name);
  if (preset?.usage) {
    return preset.usage;
  }
  return 'Usage description not registered';
}

function applyModelFilter(models, query) {
  const normalized = query.trim().toLowerCase();
  if (!normalized) return models;

  return models.filter((model) => {
    const haystack = [
      model.name,
      model.display_name,
      model.description,
      getPresetUsage(model),
      getGpuHint(model),
      computeSizeGbFromModel(model)?.toString(),
    ]
      .filter(Boolean)
      .join(' ')
      .toLowerCase();
    return haystack.includes(normalized);
  });
}

function updateModelsCount(count) {
  const target = elements.modelsCount();
  if (target) {
    target.textContent = `${count} models`;
  }
}

function translateSourceLabel(source) {
  if (!source) return 'Source: -';
  const map = {
    ollama_library: 'Source: Ollama Official Library',
    registry: 'Source: Registry',
    cache: 'Source: Cache',
  };
  return map[source] ?? `Source: ${source}`;
}

function updateModelsSourceLabel() {
  const target = elements.modelsSourceLabel();
  if (target) {
    target.textContent = translateSourceLabel(availableModelsMeta.source);
  }
}

function updateSelectedModelSummary() {
  const heroDisplay = elements.selectedModelNameDisplay();
  const title = elements.selectedModelTitle();
  const desc = elements.selectedModelDescription();
  const size = elements.selectedModelSize();
  const gpu = elements.selectedModelGpu();
  const card = elements.selectedModelCard();
  const current = availableModels.find((model) => model.name === selectedModel);

  if (!current) {
    selectedModel = null;
    if (heroDisplay) heroDisplay.textContent = 'Not selected';
    if (title) title.textContent = 'Please select a model';
    if (desc)
      desc.textContent = 'Select a model from the list on the left before choosing distribution targets.';
    if (size) size.textContent = '-';
    if (gpu) gpu.textContent = '-';
    if (card) card.classList.add('selected-model-banner--empty');
    return;
  }

  const displayName = current.display_name || current.name;
  if (heroDisplay) heroDisplay.textContent = displayName;
  if (title) title.textContent = displayName;
  if (desc) desc.textContent = current.description || getPresetUsage(current);
  const sizeGb = computeSizeGbFromModel(current);
  if (size) size.textContent = formatModelSize(sizeGb);
  if (gpu) gpu.textContent = getGpuHint(current);
  if (card) card.classList.remove('selected-model-banner--empty');
}

function updateHeroOnlineCount(agents) {
  const target = elements.modelsOnlineCount();
  if (!target) return;
  const list = Array.isArray(agents) ? agents : [];
  const online = list.filter((agent) => agent.status === 'online').length;
  target.textContent = `${online} nodes`;
}

function updateAgentSelectionSummary() {
  const summary = elements.agentsSelectionSummary();
  const hint = elements.distributeHint();
  const onlineCount = cachedAgents.filter((agent) => agent.status === 'online').length;
  const selectedCount = document.querySelectorAll(
    '#agents-for-distribution input[type="checkbox"]:checked'
  ).length;

  if (summary) {
    summary.textContent = `Online ${onlineCount} nodes / Selected ${selectedCount} nodes`;
  }
  if (hint) {
    hint.textContent =
      selectedCount === 0
        ? 'Target: Selected agents only'
        : `Target: ${selectedCount} agents`;
  }

  updateHeroOnlineCount(cachedAgents);
  return selectedCount;
}

function handleModelSearch(event) {
  const value = event?.target?.value ?? '';
  modelFilterQuery = value;
  renderAvailableModels(availableModels);
}

function renderModelCard(model) {
  const displayName = escapeHtml(model.display_name || model.name);
  const description = escapeHtml(model.description || getPresetUsage(model));
  const gpuHint = getGpuHint(model);
  const sizeGb = computeSizeGbFromModel(model);
  const sizeLabel = formatModelSize(sizeGb);
  const preset = getModelPreset(model.name);
  const badges = [];
  if (preset?.usage) {
    badges.push(`<span class="model-badge">${escapeHtml(preset.usage)}</span>`);
  }
  if (preset?.badge) {
    badges.push(`<span class="model-badge">${escapeHtml(preset.badge)}</span>`);
  }

  return `
    <div
      class="model-card ${selectedModel === model.name ? 'model-card--selected' : ''}"
      data-model-name="${escapeHtml(model.name)}"
      tabindex="0"
      role="button"
      aria-pressed="${selectedModel === model.name}"
    >
      <h4>${displayName}</h4>
      <p class="model-size">${escapeHtml(sizeLabel)} / ${escapeHtml(gpuHint)}</p>
      <p class="model-desc">${description}</p>
      ${
        badges.length
          ? `<div class="model-card__badges">
              ${badges.join('')}
            </div>`
          : ''
      }
    </div>
  `;
}

function renderHfModelCard(model) {
  const card = renderModelCard(model);
  const repo = model.repo || (model.name?.split('/').slice(1, -1).join('/') ?? '');
  const filename = model.filename || model.name?.split('/').pop() || '';
  return card.replace(
    '</div>',
    `
      <div class="model-card__actions">
        <button class="btn btn-small" data-action="pull" data-repo="${escapeHtml(
          repo
        )}" data-file="${escapeHtml(filename)}">Pull</button>
      </div>
    </div>`
  );
}

function renderRegisteredModelCard(model) {
  const card = renderModelCard(model);
  return card.replace(
    '</div>',
    `
      <div class="model-card__actions">
        <button class="btn btn-small" data-action="download" data-model="${escapeHtml(
          model.name
        )}">Download (all)</button>
      </div>
    </div>`
  );
}

function resolveGpuModel(agent) {
  if (!agent) return '';
  if (agent.gpu_model_name) return agent.gpu_model_name;
  if (agent.gpu_model) return agent.gpu_model;
  if (Array.isArray(agent.gpu_devices) && agent.gpu_devices.length > 0) {
    return agent.gpu_devices[0].model ?? '';
  }
  return '';
}

// ========== UI Update Functions ==========

/**
 * Render available models list
 */
function renderAvailableModels(models) {
  const container = elements.availableModelsList();
  if (!container) return;

  updateModelsCount(models.length);
  updateModelsSourceLabel();

  const filtered = applyModelFilter(models, modelFilterQuery);

  if (filtered.length === 0) {
    const queryText = escapeHtml(modelFilterQuery);
    container.innerHTML = `<p class="empty-message">No models matching "${queryText}"</p>`;
    updateSelectedModelSummary();
    return;
  }

  container.innerHTML = filtered.map((model) => renderModelCard(model)).join('');

  // Model selection event
  container.querySelectorAll('.model-card').forEach((card) => {
    card.addEventListener('click', () => selectModel(card.dataset.modelName));
    card.addEventListener('keypress', (e) => {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        selectModel(card.dataset.modelName);
      }
    });
  });

  updateSelectedModelSummary();
}

function renderHfModels(models) {
  const container = elements.hfModelsList();
  const status = elements.hfStatus();
  if (!container) return;
  if (status) status.textContent = availableModelsMeta.cached ? 'Cached' : '';
  if (models.length === 0) {
    container.innerHTML = '<p class="empty-message">No HF models</p>';
    return;
  }
  const filtered = applyModelFilter(models, modelFilterQuery);
  if (filtered.length === 0) {
    container.innerHTML = '<p class="empty-message">No HF models match filter</p>';
    return;
  }
  container.innerHTML = filtered.map((m) => renderHfModelCard(m)).join('');
  container.querySelectorAll('button[data-action="pull"]').forEach((btn) => {
    btn.addEventListener('click', async () => {
      const repo = btn.dataset.repo;
      const file = btn.dataset.file;
      try {
        await pullModelFromHf(repo, file, null);
        await refreshRegisteredModels();
        showSuccess('Pulled model to router');
      } catch (e) {
        console.error(e);
        showError(e.message || 'Failed to pull model');
      }
    });
  });
}

function renderRegisteredModels(models) {
  const container = elements.registeredModelsList();
  if (!container) return;
  if (models.length === 0) {
    container.innerHTML = '<p class="empty-message">No registered models</p>';
    return;
  }
  container.innerHTML = models.map((m) => renderRegisteredModelCard(m)).join('');
  container.querySelectorAll('button[data-action="download"]').forEach((btn) => {
    btn.addEventListener('click', async () => {
      const modelName = btn.dataset.model;
      await distributeModel(modelName, 'all');
      showSuccess('Download started for all nodes');
    });
  });
}

/**
 * Select a model
 */
function selectModel(modelName) {
  selectedModel = modelName;
  renderAvailableModels(availableModels);
  updateDistributeButtonState();
}

/**
 * Render agents list (distribution targets)
 */
function renderAgentsForDistribution(agents) {
  const container = elements.agentsForDistribution();
  if (!container) return;

  cachedAgents = Array.isArray(agents) ? agents : [];
  updateHeroOnlineCount(cachedAgents);

  if (cachedAgents.length === 0) {
    container.innerHTML = '<p class="empty-message">No registered agents</p>';
    updateDistributeButtonState();
    return;
  }

  container.innerHTML = cachedAgents
    .map((agent) => {
      const labelClasses = ['agent-checkbox'];
      if (agent.status !== 'online') {
        labelClasses.push('agent-checkbox--offline');
      }
      const checkboxDisabled = agent.status === 'online' ? '' : 'disabled';
      const name = escapeHtml(agent.machine_name || agent.id?.substring(0, 8) || '-');
      const gpuModel = resolveGpuModel(agent);
      const gpuLine = gpuModel ? `<span class="agent-checkbox__gpu">${escapeHtml(gpuModel)}</span>` : '';
      const statusLabel = agent.status === 'online' ? 'Online' : 'Offline';
      const statusClass = agent.status === 'online' ? 'status--online' : 'status--offline';

      return `
        <label class="${labelClasses.join(' ')}">
          <input
            type="checkbox"
            class="agent-checkbox__input"
            data-agent-id="${escapeHtml(agent.id ?? '')}"
            ${checkboxDisabled}
          />
          <span class="agent-checkbox__label">
            <strong>${name}</strong>
            <span class="agent-checkbox__status ${statusClass}">
              ${statusLabel}
            </span>
            ${gpuLine}
          </span>
        </label>
      `;
    })
    .join('');

  // On checkbox change
  container.querySelectorAll('input[type="checkbox"]').forEach((checkbox) => {
    checkbox.addEventListener('change', updateDistributeButtonState);
  });

  updateDistributeButtonState();
}

/**
 * Update distribute button enabled/disabled state
 */
function updateDistributeButtonState() {
  const selectedCount = updateAgentSelectionSummary();
  const button = elements.distributeButton();
  if (!button) return;

  const hasSelectedModel = !!selectedModel;

  // Always disabled when manual panel is closed
  const manualPanel = elements.manualPanel();
  const panelOpen = manualPanel && manualPanel.classList.contains('manual-panel--expanded');
  button.disabled = !(panelOpen && hasSelectedModel && selectedCount > 0);
}

/**
 * Render download tasks list
 */
function renderDownloadTasks() {
  const container = elements.downloadTasksList();
  if (!container) return;

  const tasks = Array.from(downloadTasks.values());

  if (tasks.length === 0) {
    container.innerHTML = '<p class="empty-message">No running tasks</p>';
    return;
  }

  container.innerHTML = tasks
    .map((task) => {
      const modelLabel = escapeHtml(task.model_name ?? '-');
      const agentLabel = task.agent_id ? escapeHtml(task.agent_id.substring(0, 8)) : '-';
      const statusClass = escapeHtml(task.status);
      const statusLabel = escapeHtml(translateStatus(task.status));
      const progressValue =
        typeof task.progress === 'number' && !Number.isNaN(task.progress) ? task.progress : 0;
      const normalizedProgress = Math.min(1, Math.max(0, progressValue));
      const speedText =
        typeof task.download_speed_bps === 'number'
          ? `<div class="task-speed">${formatSpeed(task.download_speed_bps)}</div>`
          : '';
      const progressBlock =
        task.status === 'downloading'
          ? `
        <div class="task-progress">
          <progress value="${normalizedProgress}" max="1"></progress>
          <span class="task-progress-text">${(normalizedProgress * 100).toFixed(1)}%</span>
        </div>
        ${speedText}
      `
          : '';

      return `
        <div class="task-card" data-task-id="${escapeHtml(task.id ?? '')}">
          <div class="task-header">
            <strong>${modelLabel}</strong>
            <span class="task-status task-status--${statusClass}">${statusLabel}</span>
          </div>
          <div class="task-agent">Agent: ${agentLabel}${task.agent_id ? '...' : ''}</div>
          ${progressBlock}
          <div class="task-time">Started: ${formatTimestamp(task.created_at)}</div>
        </div>
      `;
    })
    .join('');
}

/**
 * Render loaded models (aggregated across all agents)
 */
function renderLoadedModels() {
  const container = elements.loadedModelsList();
  if (!container) return;

  if (!Array.isArray(loadedModels) || loadedModels.length === 0) {
    container.innerHTML = '<p class="empty-message">No loaded models</p>';
    return;
  }

  container.innerHTML = loadedModels
    .map((item) => {
      const total = item.total_agents ?? 0;
      const completed = item.completed ?? 0;
      const downloading = item.downloading ?? 0;
      const pending = item.pending ?? 0;
      const failed = item.failed ?? 0;

      return `
        <div class="task-card">
          <div class="task-header">
            <strong>${escapeHtml(item.model_name)}</strong>
            <span class="task-status task-status--completed">Completed ${completed}</span>
          </div>
          <div class="task-agent">Total agents: ${total}</div>
          <div class="task-progress">
            <span class="task-progress-text">In progress ${downloading} / Pending ${pending} / Failed ${failed}</span>
          </div>
        </div>
      `;
    })
    .join('');
}

/**
 * Translate task status to display text
 */
function translateStatus(status) {
  const statusMap = {
    pending: 'Pending',
    downloading: 'Downloading',
    completed: 'Completed',
    failed: 'Failed',
  };
  return statusMap[status] || status;
}

/**
 * Format download speed
 */
function formatSpeed(bytesPerSec) {
  if (bytesPerSec < 1024) return `${bytesPerSec} B/s`;
  if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`;
  if (bytesPerSec < 1024 * 1024 * 1024) return `${(bytesPerSec / (1024 * 1024)).toFixed(1)} MB/s`;
  return `${(bytesPerSec / (1024 * 1024 * 1024)).toFixed(1)} GB/s`;
}

/**
 * Format timestamp
 */
function formatTimestamp(isoString) {
  if (!isoString) return '-';
  const date = new Date(isoString);
  return date.toLocaleString('en-US', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  });
}

/**
 * Show error message
 */
function showError(message) {
  const banner = document.getElementById('error-banner');
  if (banner) {
    banner.textContent = message;
    banner.classList.remove('hidden');
    setTimeout(() => banner.classList.add('hidden'), 5000);
  }
}

function showSuccess(message) {
  const banner = document.getElementById('error-banner');
  if (banner) {
    banner.textContent = message;
    banner.classList.remove('hidden');
    banner.classList.add('success-banner');
    setTimeout(() => {
      banner.classList.add('hidden');
      banner.classList.remove('success-banner');
    }, 3000);
  } else {
    console.info(message);
  }
}

// ========== Progress Monitoring ==========

/**
 * Monitor download progress every 5 seconds
 */
function monitorProgress() {
  if (progressPollingInterval) {
    clearInterval(progressPollingInterval);
  }

  progressPollingInterval = setInterval(async () => {
    const taskIds = Array.from(downloadTasks.keys());

    for (const taskId of taskIds) {
      const task = await fetchTaskProgress(taskId);
      if (task) {
        downloadTasks.set(taskId, task);

        // Remove completed or failed tasks after 10 seconds
        if (task.status === 'completed' || task.status === 'failed') {
          setTimeout(() => {
            downloadTasks.delete(taskId);
            renderDownloadTasks();
          }, 10000);
        }
      }
    }

    renderDownloadTasks();
    // Update loaded models summary
    loadedModels = await fetchLoadedModels();
    renderLoadedModels();

    // Stop polling when no tasks remain
    if (downloadTasks.size === 0) {
      clearInterval(progressPollingInterval);
      progressPollingInterval = null;
    }
  }, 5000);
}

// ========== Event Handlers ==========

function toggleManualPanel() {
  const panel = elements.manualPanel();
  const btn = elements.toggleManualBtn();
  if (!panel || !btn) return;

  manualPanelOpen = !panel.classList.contains('manual-panel--expanded');
  panel.classList.toggle('manual-panel--expanded', manualPanelOpen);
  panel.classList.toggle('manual-panel--collapsed', !manualPanelOpen);
  btn.textContent = manualPanelOpen ? 'Close manual distribution panel' : 'Open manual distribution panel';

  updateDistributeButtonState();
}

/**
 * Model distribution button click
 */
async function handleDistribute() {
  if (!selectedModel) {
    showError('Please select a model');
    return;
  }

  const checkboxes = document.querySelectorAll(
    '#agents-for-distribution input[type="checkbox"]:checked'
  );
  const agentIds = Array.from(checkboxes).map((cb) => cb.dataset.agentId);

  if (agentIds.length === 0) {
    showError('Please select agents');
    return;
  }

  const taskIds = await distributeModel(selectedModel, 'specific', agentIds);

  if (taskIds.length > 0) {
    // タスクを追加
    for (const taskId of taskIds) {
      downloadTasks.set(taskId, {
        id: taskId,
        model_name: selectedModel,
        status: 'pending',
        progress: 0,
        agent_id: agentIds[taskIds.indexOf(taskId)] || agentIds[0],
        created_at: new Date().toISOString(),
      });
    }

    renderDownloadTasks();
    monitorProgress();

    // チェックボックスをクリア
    checkboxes.forEach((cb) => (cb.checked = false));
    updateDistributeButtonState();

    showError(`Started ${taskIds.length} download task(s)`);
  }
}

/**
 * すべて選択ボタンクリック
 */
function handleSelectAll() {
  const manualPanel = elements.manualPanel();
  if (!manualPanel || manualPanel.classList.contains('manual-panel--collapsed')) return;
  const checkboxes = document.querySelectorAll(
    '#agents-for-distribution input[type="checkbox"]:not(:disabled)'
  );
  checkboxes.forEach((cb) => (cb.checked = true));
  updateDistributeButtonState();
}

/**
 * 選択解除ボタンクリック
 */
function handleDeselectAll() {
  const manualPanel = elements.manualPanel();
  if (!manualPanel || manualPanel.classList.contains('manual-panel--collapsed')) return;
  const checkboxes = document.querySelectorAll(
    '#agents-for-distribution input[type="checkbox"]'
  );
  checkboxes.forEach((cb) => (cb.checked = false));
  updateDistributeButtonState();
}

// ========== 初期化 ==========

/**
 * モデル管理UIの初期化
 */
export async function initModelsUI(agents) {
  // 利用可能なモデルを取得
  availableModels = await fetchAvailableModels();
  modelFilterQuery = '';
  renderAvailableModels(availableModels);
  renderHfModels(availableModels);

  registeredModels = await fetchRegisteredModels();
  renderRegisteredModels(registeredModels);

  // エージェント一覧を描画
  renderAgentsForDistribution(agents);

  // イベントリスナー登録
  const distributeButton = elements.distributeButton();
  const selectAllButton = elements.selectAllButton();
  const deselectAllButton = elements.deselectAllButton();
  const searchInput = elements.modelSearchInput();
  const toggleManualBtn = elements.toggleManualBtn();
  const hfSearch = elements.hfSearchInput();
  const hfRefresh = elements.hfRefreshBtn();
  const regRefresh = elements.registeredRefreshBtn();

  if (distributeButton) {
    distributeButton.addEventListener('click', handleDistribute);
  }

  if (selectAllButton) {
    selectAllButton.addEventListener('click', handleSelectAll);
  }

  if (deselectAllButton) {
    deselectAllButton.addEventListener('click', handleDeselectAll);
  }

  if (searchInput) {
    searchInput.value = '';
    searchInput.addEventListener('input', handleModelSearch);
  }

  if (hfSearch) {
    hfSearch.addEventListener('input', async () => {
      availableModels = await fetchAvailableModels();
      renderHfModels(availableModels);
    });
  }

  if (hfRefresh) {
    hfRefresh.addEventListener('click', async () => {
      availableModels = await fetchAvailableModels();
      renderHfModels(availableModels);
    });
  }

  if (regRefresh) {
    regRefresh.addEventListener('click', refreshRegisteredModels);
  }

  if (toggleManualBtn) {
    toggleManualBtn.addEventListener('click', toggleManualPanel);
  }

  // 初期状態を設定
  updateDistributeButtonState();
  renderDownloadTasks();

  // ロード済みモデルを初期取得
  loadedModels = await fetchLoadedModels();
  renderLoadedModels();

  // アクティブなタスクを初期取得
  const activeTasks = await fetchActiveTasks();
  for (const task of activeTasks) {
    downloadTasks.set(task.id, task);
  }
  renderDownloadTasks();

  // 進捗監視開始
  monitorProgress();

  const tasksRefresh = elements.tasksRefreshBtn();
  if (tasksRefresh) {
    tasksRefresh.addEventListener('click', async () => {
      // サーバーからアクティブなタスク一覧を再取得
      const tasks = await fetchActiveTasks();
      downloadTasks.clear();
      for (const task of tasks) {
        downloadTasks.set(task.id, task);
      }
      renderDownloadTasks();
    });
  }
}

/**
 * モデル管理UIの更新（エージェント一覧変更時）
 */
export function updateModelsUI(agents) {
  renderAgentsForDistribution(agents);
}

async function refreshRegisteredModels() {
  registeredModels = await fetchRegisteredModels();
  renderRegisteredModels(registeredModels);
}
