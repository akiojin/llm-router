# UI契約: 管理ダッシュボード

**機能ID**: `SPEC-712c20cf` | **日付**: 2025-10-31

## 概要

管理ダッシュボードのUI仕様とコンポーネント契約。Vanilla JS + Chart.jsによる実装を定義します。

## ページレイアウト

```
┌─────────────────────────────────────────────────────────────────┐
│ LLM Router Dashboard                         [Refresh] │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌──────────┐ │
│ │ Total       │ │ Online      │ │ Total Req   │ │ Avg RT   │ │
│ │   10        │ │    8        │ │    0        │ │  0ms     │ │
│ └─────────────┘ └─────────────┘ └─────────────┘ └──────────┘ │
│                                                                 │
├─────────────────────────────────────────────────────────────────┤
│ Agent List                                                      │
│ ┌───────────┬─────────────┬─────────┬─────────┬────────────┐  │
│ │ Name      │ IP Address  │ Status  │ Uptime  │ Action     │  │
│ ├───────────┼─────────────┼─────────┼─────────┼────────────┤  │
│ │ server-01 │ 192.168.1.1 │ Online  │ 2h 30m  │ [Details]  │  │
│ │ server-02 │ 192.168.1.2 │ Online  │ 3h 15m  │ [Details]  │  │
│ │ server-03 │ 192.168.1.3 │ Offline │   -     │ [Details]  │  │
│ └───────────┴─────────────┴─────────┴─────────┴────────────┘  │
│                                                                 │
├─────────────────────────────────────────────────────────────────┤
│ Request History (Last 1 hour)                                  │
│                                                                 │
│     Requests/min                                                │
│  30 ┤    ╭─╮                                                   │
│  20 ┤  ╭─╯ ╰╮                                                  │
│  10 ┤╭─╯    ╰─╮                                                │
│   0 ┴───────────────────────────                               │
│     12:00  12:15  12:30  12:45                                 │
└─────────────────────────────────────────────────────────────────┘
```

## コンポーネント契約

### 1. SystemStatsCard

**説明**: システム統計を表示するカードコンポーネント

**HTML構造**:
```html
<div class="stats-container">
  <div class="stat-card">
    <h3>Total Agents</h3>
    <p id="total-agents" class="stat-value">0</p>
  </div>
  <div class="stat-card">
    <h3>Online Agents</h3>
    <p id="online-agents" class="stat-value">0</p>
  </div>
  <div class="stat-card">
    <h3>Total Requests</h3>
    <p id="total-requests" class="stat-value">0</p>
  </div>
  <div class="stat-card">
    <h3>Avg Response Time</h3>
    <p id="avg-response-time" class="stat-value">0ms</p>
  </div>
</div>
```

**CSS要件**:
- カードは横並び（flexbox使用）
- レスポンシブ: 768px未満で縦並びに変更
- カード背景: 白、ボーダー: 1px solid #ddd
- パディング: 20px
- シャドウ: 0 2px 4px rgba(0,0,0,0.1)

**JavaScript契約**:
```javascript
/**
 * システム統計を更新
 * @param {Object} stats - システム統計オブジェクト
 * @param {number} stats.total_agents - 総ノード数
 * @param {number} stats.online_agents - オンラインノード数
 * @param {number} stats.total_requests - 総リクエスト数
 * @param {number} stats.avg_response_time_ms - 平均レスポンスタイム
 */
function updateStats(stats) {
  document.getElementById('total-agents').textContent = stats.total_agents;
  document.getElementById('online-agents').textContent = stats.online_agents;
  document.getElementById('total-requests').textContent = stats.total_requests;
  document.getElementById('avg-response-time').textContent = `${stats.avg_response_time_ms}ms`;
}
```

---

### 2. AgentTable

**説明**: ノード一覧を表示するテーブルコンポーネント

**HTML構造**:
```html
<div class="agent-table-container">
  <h2>Agent List</h2>
  <table class="agent-table">
    <thead>
      <tr>
        <th>Name</th>
        <th>IP Address</th>
        <th>Status</th>
        <th>Uptime</th>
        <th>Action</th>
      </tr>
    </thead>
    <tbody id="agent-table-body">
      <!-- ノード行が動的に追加される -->
    </tbody>
  </table>
</div>
```

**CSS要件**:
- テーブル幅: 100%
- ボーダー: 1px solid #ddd
- ヘッダー背景: #f5f5f5
- 行hover: 背景色 #f9f9f9
- ステータスバッジ:
  - Online: 緑色背景 (#4CAF50)、白文字
  - Offline: グレー背景 (#9E9E9E)、白文字

**JavaScript契約**:
```javascript
/**
 * ノード一覧を更新
 * @param {Array<Object>} agents - ノード配列
 * @param {string} agents[].id - ノードID
 * @param {string} agents[].machine_name - マシン名
 * @param {string} agents[].ip_address - IPアドレス
 * @param {string} agents[].status - ステータス ("Online" | "Offline")
 * @param {number} agents[].uptime_seconds - 直近オンライン開始からの稼働時間（秒）
 */
function updateAgentTable(agents) {
  const tbody = document.getElementById('agent-table-body');
  tbody.innerHTML = agents.map(agent => `
    <tr class="agent-row ${agent.status.toLowerCase()}">
      <td>${escapeHtml(agent.machine_name)}</td>
      <td>${escapeHtml(agent.ip_address)}</td>
      <td><span class="status-badge ${agent.status.toLowerCase()}">${agent.status}</span></td>
      <td>${formatUptime(agent.uptime_seconds)}</td>
      <td><button onclick="showDetails('${agent.id}')">Details</button></td>
    </tr>
  `).join('');
}

/**
 * 稼働時間をフォーマット
 * @param {number} seconds - 秒数
 * @returns {string} フォーマット済み文字列（例: "2h 30m"）
 */
function formatUptime(seconds) {
  if (seconds < 0) return '-';
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  return `${hours}h ${minutes}m`;
}

/**
 * HTMLエスケープ（XSS対策）
 * @param {string} text - エスケープ対象文字列
 * @returns {string} エスケープ済み文字列
 */
function escapeHtml(text) {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}
```

---

### 3. RequestChart

**説明**: リクエスト履歴をグラフ表示するコンポーネント（将来拡張）

**HTML構造**:
```html
<div class="chart-container">
  <h2>Request History (Last 1 hour)</h2>
  <canvas id="request-chart"></canvas>
</div>
```

**CSS要件**:
- キャンバス最大幅: 100%
- キャンバス高さ: 300px
- レスポンシブ対応

**JavaScript契約**:
```javascript
/**
 * リクエスト履歴グラフを初期化
 * @returns {Chart} Chart.jsインスタンス
 */
function initRequestChart() {
  const ctx = document.getElementById('request-chart').getContext('2d');
  return new Chart(ctx, {
    type: 'line',
    data: {
      labels: [],
      datasets: [{
        label: 'Requests/min',
        data: [],
        borderColor: '#4CAF50',
        backgroundColor: 'rgba(76, 175, 80, 0.1)',
        tension: 0.3
      }]
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      scales: {
        y: {
          beginAtZero: true
        }
      }
    }
  });
}

/**
 * グラフデータを更新
 * @param {Chart} chart - Chart.jsインスタンス
 * @param {Array<Object>} history - リクエスト履歴
 * @param {string} history[].timestamp - タイムスタンプ
 * @param {number} history[].count - リクエスト数
 */
function updateRequestChart(chart, history) {
  chart.data.labels = history.map(h => new Date(h.timestamp).toLocaleTimeString());
  chart.data.datasets[0].data = history.map(h => h.count);
  chart.update();
}
```

---

## ポーリングロジック

**ポーリング設定**:
```javascript
const POLL_INTERVAL = 5000; // 5秒

/**
 * ダッシュボードデータを取得して更新
 */
async function refreshDashboard() {
  try {
    // ノード一覧を取得
    const agentsResponse = await fetch('/api/dashboard/agents');
    if (!agentsResponse.ok) {
      throw new Error(`HTTP error! status: ${agentsResponse.status}`);
    }
    const agents = await agentsResponse.json();
    updateAgentTable(agents);

    // システム統計を取得
    const statsResponse = await fetch('/api/dashboard/stats');
    if (!statsResponse.ok) {
      throw new Error(`HTTP error! status: ${statsResponse.status}`);
    }
    const stats = await statsResponse.json();
    updateStats(stats);
  } catch (error) {
    console.error('Failed to refresh dashboard:', error);
    showError('データの取得に失敗しました。');
  }
}

/**
 * 定期的にダッシュボードを更新
 */
function startPolling() {
  setInterval(refreshDashboard, POLL_INTERVAL);
}

/**
 * 初期化処理
 */
window.addEventListener('DOMContentLoaded', async () => {
  // 初回ロード
  await refreshDashboard();

  // ポーリング開始
  startPolling();

  // リフレッシュボタン
  document.getElementById('refresh-btn').addEventListener('click', refreshDashboard);
});
```

---

## エラーハンドリング

**エラー表示**:
```javascript
/**
 * エラーメッセージを表示
 * @param {string} message - エラーメッセージ
 */
function showError(message) {
  const errorDiv = document.getElementById('error-message');
  errorDiv.textContent = message;
  errorDiv.style.display = 'block';

  // 5秒後に自動的に非表示
  setTimeout(() => {
    errorDiv.style.display = 'none';
  }, 5000);
}
```

**HTML構造**:
```html
<div id="error-message" class="error-message" style="display: none;"></div>
```

**CSS要件**:
- 背景色: #f44336（赤）
- 文字色: 白
- パディング: 16px
- ボーダーラジアス: 4px
- 位置: fixed、top: 20px、right: 20px

---

## レスポンシブデザイン

### ブレークポイント

**デスクトップ（1024px以上）**:
- 統計カード: 4列
- テーブル: 全カラム表示

**タブレット（768px - 1023px）**:
- 統計カード: 2列
- テーブル: 全カラム表示

**モバイル（767px以下）**:
- 統計カード: 1列
- テーブル: スクロール可能
- 詳細ボタンのみ表示、他は折りたたみ

**CSS例**:
```css
@media (max-width: 767px) {
  .stats-container {
    flex-direction: column;
  }

  .agent-table-container {
    overflow-x: auto;
  }

  .agent-table {
    min-width: 600px;
  }
}
```

---

## パフォーマンス要件

- **初回ロード時間**: < 2秒（NFR-011）
- **ポーリングオーバーヘッド**: < 100ms（NFR-011）
- **JavaScriptファイルサイズ**: < 50KB（gzip圧縮後）
- **CSSファイルサイズ**: < 20KB（gzip圧縮後）

---

## セキュリティ要件

- **XSS対策**: すべての動的コンテンツをHTMLエスケープ
- **CSP**: Content Security Policyヘッダー設定
  ```
  Content-Security-Policy: default-src 'self'; script-src 'self' https://cdn.jsdelivr.net; style-src 'self' 'unsafe-inline'
  ```
- **CORS**: 同一オリジンのみ許可

---

## テスト契約

### E2Eテスト要件
1. ダッシュボードページが表示される
2. ノード一覧が正しく表示される
3. システム統計が正しく表示される
4. リフレッシュボタンが動作する
5. ポーリングが5秒ごとに実行される
6. エラーメッセージが適切に表示される

### 手動テスト要件
1. 各ブラウザで表示確認（Chrome, Firefox, Safari, Edge）
2. レスポンシブデザイン確認（デスクトップ、タブレット、モバイル）
3. アクセシビリティ確認（スクリーンリーダー、キーボード操作）

---

*このUI契約は plan.md Phase 1 の成果物です*
