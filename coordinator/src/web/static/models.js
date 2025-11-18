/**
 * モデル管理UI
 *
 * モデル配布、進捗追跡、モデル情報表示を管理
 */

// ========== 定数 ==========
const MODEL_PRESETS = {
  'gpt-oss:20b': {
    gpuHint: '推奨GPU: 16GB以上',
    badge: 'GPT-OSS 20B',
    usage: '高精度・長文向けの汎用/コード両対応',
    sizeGb: 14.5,
  },
  'gpt-oss-safeguard:20b': {
    gpuHint: '推奨GPU: 16GB以上',
    badge: 'Safety 20B',
    usage: 'モデレーション・セーフティ判定用',
    sizeGb: 14,
  },
  'gpt-oss:120b': {
    gpuHint: '推奨GPU: 80GB以上',
    badge: 'GPT-OSS 120B',
    usage: '最高精度だが超大規模GPU向け',
    sizeGb: 65,
  },
  'qwen3-coder:30b': {
    gpuHint: '推奨GPU: 24GB以上',
    badge: 'Qwen3 Coder 30B',
    usage: '最新世代のコード生成',
    sizeGb: 17,
  },
};

// ========== 状態管理 ==========
let availableModels = [];
let availableModelsMeta = { source: null };
let selectedModel = null;
let downloadTasks = new Map(); // task_id -> task info
let progressPollingInterval = null;
let modelFilterQuery = '';
let cachedAgents = [];
let loadedModels = [];
let manualPanelOpen = false;

// ========== DOM要素取得 ==========
const elements = {
  availableModelsList: () => document.getElementById('available-models-list'),
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
};

// ========== API関数 ==========

/**
 * GET /api/models/available - 利用可能なモデル一覧を取得
 */
async function fetchAvailableModels() {
  try {
    const response = await fetch('/api/models/available');
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    }
    const data = await response.json();
    availableModelsMeta.source = data.source ?? null;
    return data.models || [];
  } catch (error) {
    console.error('Failed to fetch available models:', error);
    showError('利用可能なモデルの取得に失敗しました');
    return [];
  }
}

/**
 * POST /api/models/distribute - モデルを配布
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
    showError(`モデル配布に失敗しました: ${error.message}`);
    return [];
  }
}

/**
 * GET /api/agents/{agent_id}/models - エージェントのインストール済みモデル取得
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
 * GET /api/tasks/{task_id} - タスク進捗を取得
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
 * GET /api/models/loaded - コーディネーター全体のロード済みモデル集計
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

// ========== ヘルパー ==========

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
    return 'サイズ不明';
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
  // フォールバック: 必要メモリから概算
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
    // 推奨メモリ情報がない場合はサイズ相当で代替
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
    if (basis >= 12) return '推奨GPU: 16GB以上';
    if (basis >= 8) return '推奨GPU: 8〜16GB';
    if (basis >= 4.5) return '推奨GPU: 4.5〜8GB';
    return '推奨GPU: 4.5GB未満';
  }
  return '推奨GPU: 情報未提供';
}

function getPresetUsage(model) {
  const preset = getModelPreset(model.name);
  if (preset?.usage) {
    return preset.usage;
  }
  return '用途の説明が登録されていません';
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
    target.textContent = `${count}件`;
  }
}

function translateSourceLabel(source) {
  if (!source) return 'ソース: -';
  const map = {
    ollama_library: 'ソース: Ollama公式ライブラリ',
    registry: 'ソース: レジストリ',
    cache: 'ソース: キャッシュ',
  };
  return map[source] ?? `ソース: ${source}`;
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
    if (heroDisplay) heroDisplay.textContent = '未選択';
    if (title) title.textContent = 'モデルを選択してください';
    if (desc)
      desc.textContent = '配布対象を選ぶ前に、左のリストからモデルを選択してください。';
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
  target.textContent = `${online}台`;
}

function updateAgentSelectionSummary() {
  const summary = elements.agentsSelectionSummary();
  const hint = elements.distributeHint();
  const onlineCount = cachedAgents.filter((agent) => agent.status === 'online').length;
  const selectedCount = document.querySelectorAll(
    '#agents-for-distribution input[type="checkbox"]:checked'
  ).length;

  if (summary) {
    summary.textContent = `オンライン ${onlineCount}台 / 選択中 ${selectedCount}台`;
  }
  if (hint) {
    hint.textContent =
      selectedCount === 0
        ? '配布対象: 選択したエージェントのみ'
        : `配布対象: ${selectedCount}台のエージェント`;
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

function resolveGpuModel(agent) {
  if (!agent) return '';
  if (agent.gpu_model_name) return agent.gpu_model_name;
  if (agent.gpu_model) return agent.gpu_model;
  if (Array.isArray(agent.gpu_devices) && agent.gpu_devices.length > 0) {
    return agent.gpu_devices[0].model ?? '';
  }
  return '';
}

// ========== UI更新関数 ==========

/**
 * 利用可能なモデル一覧を描画
 */
function renderAvailableModels(models) {
  const container = elements.availableModelsList();
  if (!container) return;

  updateModelsCount(models.length);
  updateModelsSourceLabel();

  if (models.length === 0) {
    container.innerHTML = '<p class="empty-message">モデルが見つかりません</p>';
    updateSelectedModelSummary();
    return;
  }

  const filtered = applyModelFilter(models, modelFilterQuery);

  if (filtered.length === 0) {
    const queryText = escapeHtml(modelFilterQuery);
    container.innerHTML = `<p class="empty-message">「${queryText}」に一致するモデルはありません</p>`;
    updateSelectedModelSummary();
    return;
  }

  container.innerHTML = filtered.map((model) => renderModelCard(model)).join('');

  // モデル選択イベント
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

/**
 * モデルを選択
 */
function selectModel(modelName) {
  selectedModel = modelName;
  renderAvailableModels(availableModels);
  updateDistributeButtonState();
}

/**
 * エージェント一覧（配布先選択）を描画
 */
function renderAgentsForDistribution(agents) {
  const container = elements.agentsForDistribution();
  if (!container) return;

  cachedAgents = Array.isArray(agents) ? agents : [];
  updateHeroOnlineCount(cachedAgents);

  if (cachedAgents.length === 0) {
    container.innerHTML = '<p class="empty-message">登録済みのエージェントがありません</p>';
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
      const statusLabel = agent.status === 'online' ? 'オンライン' : 'オフライン';
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

  // チェックボックス変更時
  container.querySelectorAll('input[type="checkbox"]').forEach((checkbox) => {
    checkbox.addEventListener('change', updateDistributeButtonState);
  });

  updateDistributeButtonState();
}

/**
 * 配布ボタンの有効/無効を更新
 */
function updateDistributeButtonState() {
  const selectedCount = updateAgentSelectionSummary();
  const button = elements.distributeButton();
  if (!button) return;

  const hasSelectedModel = !!selectedModel;

  // 手動パネルが閉じているときは常に無効
  const manualPanel = elements.manualPanel();
  const panelOpen = manualPanel && manualPanel.classList.contains('manual-panel--expanded');
  button.disabled = !(panelOpen && hasSelectedModel && selectedCount > 0);
}

/**
 * ダウンロードタスク一覧を描画
 */
function renderDownloadTasks() {
  const container = elements.downloadTasksList();
  if (!container) return;

  const tasks = Array.from(downloadTasks.values());

  if (tasks.length === 0) {
    container.innerHTML = '<p class="empty-message">実行中のタスクはありません</p>';
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
          <div class="task-agent">エージェント: ${agentLabel}${task.agent_id ? '…' : ''}</div>
          ${progressBlock}
          <div class="task-time">開始: ${formatTimestamp(task.created_at)}</div>
        </div>
      `;
    })
    .join('');
}

/**
 * ロード済みモデル（全エージェント合算）を描画
 */
function renderLoadedModels() {
  const container = elements.loadedModelsList();
  if (!container) return;

  if (!Array.isArray(loadedModels) || loadedModels.length === 0) {
    container.innerHTML = '<p class="empty-message">ロード済みモデルはありません</p>';
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
            <span class="task-status task-status--completed">完了 ${completed}</span>
          </div>
          <div class="task-agent">合計エージェント: ${total}</div>
          <div class="task-progress">
            <span class="task-progress-text">進行中 ${downloading} / 待機 ${pending} / 失敗 ${failed}</span>
          </div>
        </div>
      `;
    })
    .join('');
}

/**
 * タスクステータスを日本語に変換
 */
function translateStatus(status) {
  const statusMap = {
    pending: '待機中',
    downloading: 'ダウンロード中',
    completed: '完了',
    failed: '失敗',
  };
  return statusMap[status] || status;
}

/**
 * ダウンロード速度をフォーマット
 */
function formatSpeed(bytesPerSec) {
  if (bytesPerSec < 1024) return `${bytesPerSec} B/s`;
  if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`;
  if (bytesPerSec < 1024 * 1024 * 1024) return `${(bytesPerSec / (1024 * 1024)).toFixed(1)} MB/s`;
  return `${(bytesPerSec / (1024 * 1024 * 1024)).toFixed(1)} GB/s`;
}

/**
 * タイムスタンプをフォーマット
 */
function formatTimestamp(isoString) {
  if (!isoString) return '-';
  const date = new Date(isoString);
  return date.toLocaleString('ja-JP', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  });
}

/**
 * エラーメッセージを表示
 */
function showError(message) {
  const banner = document.getElementById('error-banner');
  if (banner) {
    banner.textContent = message;
    banner.classList.remove('hidden');
    setTimeout(() => banner.classList.add('hidden'), 5000);
  }
}

// ========== 進捗監視 ==========

/**
 * ダウンロード進捗を5秒ごとに監視
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

        // 完了または失敗したタスクは10秒後に削除
        if (task.status === 'completed' || task.status === 'failed') {
          setTimeout(() => {
            downloadTasks.delete(taskId);
            renderDownloadTasks();
          }, 10000);
        }
      }
    }

    renderDownloadTasks();
    // ロード済みモデル集計を更新
    loadedModels = await fetchLoadedModels();
    renderLoadedModels();

    // タスクがなくなったらポーリング停止
    if (downloadTasks.size === 0) {
      clearInterval(progressPollingInterval);
      progressPollingInterval = null;
    }
  }, 5000);
}

// ========== イベントハンドラ ==========

function toggleManualPanel() {
  const panel = elements.manualPanel();
  const btn = elements.toggleManualBtn();
  if (!panel || !btn) return;

  manualPanelOpen = !panel.classList.contains('manual-panel--expanded');
  panel.classList.toggle('manual-panel--expanded', manualPanelOpen);
  panel.classList.toggle('manual-panel--collapsed', !manualPanelOpen);
  btn.textContent = manualPanelOpen ? '手動配布パネルを閉じる' : '手動配布パネルを開く';

  updateDistributeButtonState();
}

/**
 * モデル配布ボタンクリック
 */
async function handleDistribute() {
  if (!selectedModel) {
    showError('モデルを選択してください');
    return;
  }

  const checkboxes = document.querySelectorAll(
    '#agents-for-distribution input[type="checkbox"]:checked'
  );
  const agentIds = Array.from(checkboxes).map((cb) => cb.dataset.agentId);

  if (agentIds.length === 0) {
    showError('エージェントを選択してください');
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

    showError(`${taskIds.length}件のダウンロードタスクを開始しました`);
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

  // エージェント一覧を描画
  renderAgentsForDistribution(agents);

  // イベントリスナー登録
  const distributeButton = elements.distributeButton();
  const selectAllButton = elements.selectAllButton();
  const deselectAllButton = elements.deselectAllButton();
  const searchInput = elements.modelSearchInput();
  const toggleManualBtn = elements.toggleManualBtn();

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

  if (toggleManualBtn) {
    toggleManualBtn.addEventListener('click', toggleManualPanel);
  }

  // 初期状態を設定
  updateDistributeButtonState();
  renderDownloadTasks();

  // ロード済みモデルを初期取得
  loadedModels = await fetchLoadedModels();
  renderLoadedModels();
}

/**
 * モデル管理UIの更新（エージェント一覧変更時）
 */
export function updateModelsUI(agents) {
  renderAgentsForDistribution(agents);
}
