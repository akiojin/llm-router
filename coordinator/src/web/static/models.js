/**
 * モデル管理UI
 *
 * モデル配布、進捗追跡、モデル情報表示を管理
 */

// ========== 状態管理 ==========
let availableModels = [];
let selectedModel = null;
let downloadTasks = new Map(); // task_id -> task info
let progressPollingInterval = null;

// ========== DOM要素取得 ==========
const elements = {
  availableModelsList: () => document.getElementById('available-models-list'),
  agentsForDistribution: () => document.getElementById('agents-for-distribution'),
  distributeButton: () => document.getElementById('distribute-model-button'),
  selectAllButton: () => document.getElementById('select-all-agents'),
  deselectAllButton: () => document.getElementById('deselect-all-agents'),
  downloadTasksList: () => document.getElementById('download-tasks-list'),
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

// ========== UI更新関数 ==========

/**
 * 利用可能なモデル一覧を描画
 */
function renderAvailableModels(models) {
  const container = elements.availableModelsList();
  if (!container) return;

  if (models.length === 0) {
    container.innerHTML = '<p class="empty-message">モデルが見つかりません</p>';
    return;
  }

  container.innerHTML = models
    .map(
      (model, index) => `
    <div
      class="model-card ${selectedModel === model.name ? 'model-card--selected' : ''}"
      data-model-name="${model.name}"
      tabindex="0"
      role="button"
      aria-pressed="${selectedModel === model.name}"
    >
      <h4>${model.display_name || model.name}</h4>
      <p class="model-size">${model.size_gb ? `${model.size_gb.toFixed(1)} GB` : 'サイズ不明'}</p>
      <p class="model-desc">${model.description || ''}</p>
    </div>
  `
    )
    .join('');

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

  if (agents.length === 0) {
    container.innerHTML = '<p class="empty-message">オンラインのエージェントがありません</p>';
    return;
  }

  container.innerHTML = agents
    .map(
      (agent) => `
    <label class="agent-checkbox">
      <input
        type="checkbox"
        class="agent-checkbox__input"
        data-agent-id="${agent.id}"
        ${agent.status === 'online' ? '' : 'disabled'}
      />
      <span class="agent-checkbox__label">
        <strong>${agent.machine_name || agent.id.substring(0, 8)}</strong>
        <span class="agent-checkbox__status ${agent.status === 'online' ? 'status--online' : 'status--offline'}">
          ${agent.status === 'online' ? 'オンライン' : 'オフライン'}
        </span>
        ${agent.gpu_model ? `<span class="agent-checkbox__gpu">${agent.gpu_model}</span>` : ''}
      </span>
    </label>
  `
    )
    .join('');

  // チェックボックス変更時
  container.querySelectorAll('input[type="checkbox"]').forEach((checkbox) => {
    checkbox.addEventListener('change', updateDistributeButtonState);
  });
}

/**
 * 配布ボタンの有効/無効を更新
 */
function updateDistributeButtonState() {
  const button = elements.distributeButton();
  if (!button) return;

  const hasSelectedModel = !!selectedModel;
  const hasSelectedAgents =
    document.querySelectorAll('#agents-for-distribution input[type="checkbox"]:checked').length >
    0;

  button.disabled = !(hasSelectedModel && hasSelectedAgents);
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
    .map(
      (task) => `
    <div class="task-card" data-task-id="${task.id}">
      <div class="task-header">
        <strong>${task.model_name}</strong>
        <span class="task-status task-status--${task.status}">${translateStatus(task.status)}</span>
      </div>
      <div class="task-agent">エージェント: ${task.agent_id.substring(0, 8)}...</div>
      ${
        task.status === 'downloading'
          ? `
        <div class="task-progress">
          <progress value="${task.progress}" max="1"></progress>
          <span class="task-progress-text">${(task.progress * 100).toFixed(1)}%</span>
        </div>
        ${
          task.download_speed_bps
            ? `<div class="task-speed">${formatSpeed(task.download_speed_bps)}</div>`
            : ''
        }
      `
          : ''
      }
      <div class="task-time">開始: ${formatTimestamp(task.created_at)}</div>
    </div>
  `
    )
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

    // タスクがなくなったらポーリング停止
    if (downloadTasks.size === 0) {
      clearInterval(progressPollingInterval);
      progressPollingInterval = null;
    }
  }, 5000);
}

// ========== イベントハンドラ ==========

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
  renderAvailableModels(availableModels);

  // エージェント一覧を描画
  renderAgentsForDistribution(agents);

  // イベントリスナー登録
  const distributeButton = elements.distributeButton();
  const selectAllButton = elements.selectAllButton();
  const deselectAllButton = elements.deselectAllButton();

  if (distributeButton) {
    distributeButton.addEventListener('click', handleDistribute);
  }

  if (selectAllButton) {
    selectAllButton.addEventListener('click', handleSelectAll);
  }

  if (deselectAllButton) {
    deselectAllButton.addEventListener('click', handleDeselectAll);
  }

  // 初期状態を設定
  updateDistributeButtonState();
  renderDownloadTasks();
}

/**
 * モデル管理UIの更新（エージェント一覧変更時）
 */
export function updateModelsUI(agents) {
  renderAgentsForDistribution(agents);
}
