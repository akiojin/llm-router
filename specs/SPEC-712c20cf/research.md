# 技術リサーチ: 管理ダッシュボード

**SPEC-ID**: SPEC-712c20cf
**日付**: 2025-10-31

## リサーチ課題

1. Chart.js のバージョンと使用方法
2. ポーリング実装パターン
3. Axumでの静的ファイル配信方法
4. レスポンシブデザイン実装

## リサーチ結果

### 1. Chart.js 4.x をCDN経由で使用

**決定**: Chart.js 4.x を CDN 経由で使用

**理由**:
- ビルドプロセス不要
- シンプルで軽量（~200KB）
- 豊富なドキュメントとコミュニティサポート
- MIT License

**代替案検討**:
- **D3.js**: 強力だが学習コスト高い、オーバースペック、カスタマイズ必要
- **Recharts**: Reactが必要、憲章の「フレームワーク回避」原則に反する
- **ECharts**: 中国製、ドキュメントが英語で不十分な部分がある

**実装詳細**:
```html
<script src="https://cdn.jsdelivr.net/npm/chart.js@4.4.0/dist/chart.umd.min.js"></script>
```

**使用例**:
```javascript
const ctx = document.getElementById('agentChart');
new Chart(ctx, {
  type: 'bar',
  data: {
    labels: ['Online', 'Offline'],
    datasets: [{
      label: 'Agents',
      data: [onlineCount, offlineCount],
      backgroundColor: ['#4CAF50', '#F44336']
    }]
  }
});
```

### 2. ポーリング（5秒間隔）でリアルタイム更新

**決定**: setInterval + fetch API による5秒間隔ポーリング

**理由**:
- 実装が非常にシンプル
- WebSocketは過剰（双方向通信不要）
- 管理ダッシュボードは低頻度アクセス（~10人の管理者）
- サーバー負荷が低い（5秒×10人=2 req/s）

**代替案検討**:
- **WebSocket**: 双方向通信は不要、実装複雑化、サーバー負荷増加
- **Server-Sent Events (SSE)**: ブラウザ互換性に課題、IEサポートなし
- **Long Polling**: 無駄な接続維持、サーバーリソース消費

**実装詳細**:
```javascript
const POLL_INTERVAL = 5000; // 5秒

async function fetchDashboardData() {
  const [agents, stats] = await Promise.all([
    fetch('/api/dashboard/agents').then(r => r.json()),
    fetch('/api/dashboard/stats').then(r => r.json())
  ]);
  return { agents, stats };
}

function startPolling() {
  // 初回実行
  updateDashboard();

  // 5秒ごとに更新
  setInterval(async () => {
    const data = await fetchDashboardData();
    updateCharts(data);
    updateTable(data.agents);
  }, POLL_INTERVAL);
}
```

**エラーハンドリング**:
```javascript
setInterval(async () => {
  try {
    const data = await fetchDashboardData();
    updateDashboard(data);
    clearError();
  } catch (error) {
    showError('Failed to fetch dashboard data: ' + error.message);
  }
}, POLL_INTERVAL);
```

### 3. Axum + tower_http で静的ファイル配信

**決定**: Axum + tower_http::services::ServeDir

**理由**:
- 既存のAxumスタックに統合、追加依存最小限
- 単一バイナリ配布が可能
- 開発・本番環境で同じ構成

**代替案検討**:
- **別途Nginxで配信**: インフラ複雑化、単一バイナリ配布を阻害、憲章の「シンプルさ」に反する
- **include_str!/embed静的ファイル**: 開発時の変更が面倒、リビルド必要
- **actix-files**: 異なるWebフレームワーク、既存コードベースと不整合

**実装詳細**:
```rust
use axum::{Router, routing::get};
use tower_http::services::ServeDir;

let app = Router::new()
    // API エンドポイント
    .route("/api/dashboard/agents", get(get_agents))
    .route("/api/dashboard/stats", get(get_stats))
    // 静的ファイル配信
    .nest_service("/dashboard", ServeDir::new("coordinator/src/dashboard/static"))
    // フォールバック: /dashboard/ → /dashboard/index.html
    .fallback_service(ServeDir::new("coordinator/src/dashboard/static").fallback(ServeFile::new("coordinator/src/dashboard/static/index.html")));
```

**Cargo.toml追加**:
```toml
[dependencies]
tower-http = { version = "0.5", features = ["fs"] }
```

### 4. CSS Grid + Flexbox でレスポンシブ対応

**決定**: CSS Grid + Flexbox + Media Queries

**理由**:
- モダンCSS、フレームワーク不要
- メンテナンス容易
- ファイルサイズ小さい
- すべてのモダンブラウザでサポート

**代替案検討**:
- **Bootstrap**: 不要な機能多い（~200KB）、サイズ大きい、カスタマイズ困難
- **Tailwind CSS**: ビルドプロセス必要、憲章の「ビルドプロセス回避」に反する
- **Foundation**: Bootstrapと同様、過剰

**実装詳細**:
```css
/* メインレイアウト */
.dashboard-container {
  display: grid;
  grid-template-areas:
    "header header"
    "stats  stats"
    "table  table";
  gap: 1rem;
  padding: 1rem;
}

/* 統計カード */
.stats-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
  gap: 1rem;
}

/* エージェントテーブル */
.agent-table {
  width: 100%;
  overflow-x: auto;
}

/* モバイル対応 */
@media (max-width: 768px) {
  .dashboard-container {
    grid-template-areas:
      "header"
      "stats"
      "table";
  }

  .stats-grid {
    grid-template-columns: 1fr;
  }

  .agent-table {
    font-size: 0.875rem;
  }
}

/* タブレット対応 */
@media (min-width: 769px) and (max-width: 1024px) {
  .stats-grid {
    grid-template-columns: repeat(2, 1fr);
  }
}
```

## 結論

すべての技術選択は憲章の「シンプルさ」原則に準拠：
- ✅ ビルドプロセスなし
- ✅ 外部依存最小限（Chart.js のみ）
- ✅ フレームワーク不使用
- ✅ 直接的な実装
- ✅ メンテナンス容易

**次ステップ**: Phase 1（設計＆契約）に進む
