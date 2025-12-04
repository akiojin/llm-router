# リサーチドキュメント: ルーター主導のモデル自動配布機能

**機能ID**: `SPEC-8ae67d67` | **日付**: 2025-11-12

## 調査概要

本ドキュメントは、Phase 0で実施した技術調査の結果をまとめたものです。

---

## 1. LLM runtime公式ライブラリAPI調査

### 調査タスク
「LLM runtime Library APIからダウンロード可能なモデル一覧を取得する方法を調査」

### 調査結果

#### LLM runtimeローカ

ルAPI（既存）

**エンドポイント**: `GET /api/tags`

- **用途**: ノードにインストール済みのモデル一覧を取得
- **レスポンス形式**:
  ```json
  {
    "models": [
      {
        "name": "deepseek-r1:latest",
        "model": "deepseek-r1:latest",
        "modified_at": "2025-05-10T08:06:48.639712648-07:00",
        "size": 4683075271,
        "digest": "0a8c266910232fd3291e71e5ba1e058cc5af9d411192cf88b6d30e92b6e73163",
        "details": {
          "parent_model": "",
          "format": "gguf",
          "family": "qwen2",
          "families": ["qwen2"],
          "parameter_size": "7.6B",
          "quantization_level": "Q4_K_M"
        }
      }
    ]
  }
  ```
- **認証**: 不要（ローカルAPI）
- **レート制限**: なし

**既存実装**: `agent/src/runtime.rs` の `list_models()` メソッドで利用可能

#### LLM runtime公式ライブラリWebサイト

- **URL**: <https://runtime.com/library>
- **構造**: 静的なWebページ（モデルカタログ）
- **API**: 公開APIエンドポイントは提供されていない

#### 決定: フォールバック戦略

**主要アプローチ**:
1. **ノード経由でモデル一覧を取得**:
   - 各ノードの `GET /api/tags` エンドポイントを呼び出し
   - インストール済みモデルを集約して「利用可能なモデル」とする

2. **事前定義モデルリストの使用**:
   - よく使われるモデル名を静的リストとして定義
   - 例: `gpt-oss:20b`, `gpt-oss:7b`, `gpt-oss:3b`, `gpt-oss:1b`, `llama3.2`, `deepseek-r1` など

3. **手動入力の許可**:
   - ダッシュボードでユーザーが任意のモデル名を入力できる
   - LLM runtime Pullエンドポイントが成功すればOK

**理由**:
- LLM runtime公式ライブラリAPIが公開されていないため
- 実用上、既存ノードからモデル情報を取得することで十分
- 新しいモデルは手動指定で対応可能

**代替案が却下された理由**:
- Webスクレイピング: 不安定、メンテナンスコスト高、規約違反のリスク
- サードパーティAPI: 依存関係増加、信頼性不明

---

## 2. リアルタイム進捗更新方式調査

### 調査タスク
「リアルタイム進捗更新のプロトコルと実装パターンを調査」

### 調査結果

#### 既存ダッシュボードの実装

`coordinator/src/web/static/app.js`:
- **ポーリング方式**: 5秒間隔（`REFRESH_INTERVAL_MS = 5000`）
- **状態管理**: JavaScriptの `state` オブジェクト
- **パフォーマンス監視**:
  - `fetch`: 2000ms閾値
  - `render`: 100ms閾値
  - `backend`: 100ms閾値
- **キャッシュ戦略**: `rowCache`, `agentMetricsCache` で差分更新

#### 検討したプロトコル

| 方式 | メリット | デメリット | 判定 |
|------|---------|----------|------|
| **Long Polling** | シンプル、既存実装と統一可能 | 常時接続でリソース消費 | ✅ 採用 |
| **WebSocket** | 双方向通信、リアルタイム性高 | 複雑、axumで追加実装必要 | ❌ オーバースペック |
| **Server-Sent Events** | 一方向で軽量、HTTP/2対応 | ブラウザ互換性に課題 | △ 将来検討 |

#### 決定: Long Polling（既存パターン踏襲）

**実装方法**:
1. **APIエンドポイント**: `GET /api/tasks/{task_id}`
   - レスポンス: `DownloadTask` JSON
   - 5秒間隔でポーリング

2. **ダッシュボードUI**:
   - 既存の `setInterval` パターンを使用
   - `state` オブジェクトに `downloadTasks: Map<Uuid, DownloadTask>` を追加
   - プログレスバー表示（HTML5 `<progress>` タグ）

3. **効率化**:
   - 完了したタスクはポーリング停止
   - 複数タスクをバッチ取得: `GET /api/tasks?ids=uuid1,uuid2,...`

**理由**:
- 既存ダッシュボードと設計統一
- シンプルで保守しやすい
- 進捗更新頻度（5秒）は十分実用的

**代替案が却下された理由**:
- WebSocket: ルーターとノード間の通信はREST APIベースであり、WebSocketは設計の一貫性を損なう
- SSE: ブラウザ互換性とaxum実装の複雑さがメリットを上回らない

---

## 3. モデルダウンロードタスク管理調査

### 調査タスク
「Rustでの非同期タスクキューとスケジューリングのパターンを調査」

### 調査結果

#### 既存のモデルプル実装

`agent/src/runtime.rs` の `pull_model()` メソッド:
- **tokio非同期**: `async fn` で実装
- **ストリーミング**: `response.bytes_stream()` でNDJSON受信
- **リトライ**: `retry_http_request()` で指数バックオフ
- **進捗表示**: `indicatif` crateでプログレスバー

#### 検討したタスク管理パターン

| パターン | 実装方法 | 適用可否 |
|---------|---------|---------|
| **tokio::spawn + Arc<Mutex<HashMap>>** | タスクをspawnし、共有状態で管理 | ✅ シンプル、採用 |
| **tokio::sync::mpsc チャネル** | ワーカースレッドプールパターン | △ オーバーエンジニアリング |
| **async-std Task** | 代替ランタイム | ❌ 既存はtokio |
| **crossbeam チャネル** | スレッドベース | ❌ async不要 |

#### 決定: tokio::spawn + Arc<Mutex<HashMap>>

**データ構造**:
```rust
pub struct DownloadTaskManager {
    tasks: Arc<Mutex<HashMap<Uuid, DownloadTask>>>,
}

pub struct DownloadTask {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub model_name: String,
    pub status: DownloadStatus,
    pub progress: f32,  // 0.0-1.0
    pub speed: Option<u64>,  // bytes/sec
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
}

pub enum DownloadStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}
```

**実装フロー**:
1. **タスク作成**: `POST /api/models/distribute` でタスクをHashMapに登録
2. **非同期実行**: `tokio::spawn` でノードへのHTTPリクエストを実行
3. **進捗更新**: ストリーミングレスポンスをパースし、HashMap内のタスクを更新
4. **状態取得**: `GET /api/tasks/{id}` でHashMapから取得

**同時ダウンロード制限**:
- `tokio::sync::Semaphore` で同時実行数を10に制限
- 環境変数 `MAX_CONCURRENT_DOWNLOADS` で設定可能

**理由**:
- tokioの標準パターンで実装が容易
- 既存コードベースとの一貫性
- 憲章「シンプルさ」原則に準拠

**代替案が却下された理由**:
- mpscチャネル: ワーカープールは複雑さを増すが、メリットが少ない
- 外部キューライブラリ（sidekiq等）: ローカルメモリ管理で十分、依存関係を増やす必要なし

---

## 4. GPU能力判定ロジック調査

### 調査タスク
「既存のGPUメモリ検出コードを確認し、モデル選択マッピングを設計」

### 調査結果

#### 既存のGPU情報収集

`agent/src/metrics.rs` の `MetricsCollector`:
- **GPU検出**: `gpu_devices()` メソッド
- **メモリ情報**: `gpu_memory_total_mb`, `gpu_memory_used_mb`
- **GPU能力スコア**: `GpuCapability::score()` メソッド（0-10000）

`agent/src/main.rs` の登録処理:
```rust
let gpu_devices = metrics_collector.gpu_devices();
let total_gpu_count: u32 = gpu_devices.iter().map(|device| device.count).sum();
let primary_gpu_model = gpu_devices.first().map(|device| device.model.clone());

let register_req = RegisterRequest {
    gpu_available: true,
    gpu_devices: gpu_devices.clone(),
    gpu_count: Some(total_gpu_count),
    gpu_model: primary_gpu_model,
    // ...
};
```

#### 既存のモデルメモリ要件

`agent/src/runtime.rs`:
```rust
const DEFAULT_MODEL_CANDIDATES: &[&str] =
    &["gpt-oss:20b", "gpt-oss:7b", "gpt-oss:3b", "gpt-oss:1b"];

const MODEL_MEMORY_REQUIREMENTS: &[(&str, f64)] = &[
    ("gpt-oss:20b", 12.0),  // 12 GB
    ("gpt-oss:7b", 6.0),    // 6 GB
    ("gpt-oss:3b", 3.0),    // 3 GB
    ("gpt-oss:1b", 1.0),    // 1 GB
];
```

#### 決定: GPUメモリベースのモデル選択ロジック

**マッピング**:
| GPUメモリサイズ | 推奨モデル | 理由 |
|----------------|-----------|------|
| ≥ 16 GB | `gpt-oss:20b` | 大規模モデル実行可能 |
| 8 GB - 16 GB | `gpt-oss:7b` | 中規模モデル最適 |
| 4.5 GB - 8 GB | `gpt-oss:3b` | 小規模モデル |
| < 4.5 GB | `gpt-oss:1b` | 最小モデル |

**実装**:
```rust
fn select_model_by_gpu_memory(gpu_memory_mb: u64) -> &'static str {
    let gpu_memory_gb = gpu_memory_mb as f64 / 1024.0;

    if gpu_memory_gb >= 16.0 {
        "gpt-oss:20b"
    } else if gpu_memory_gb >= 8.0 {
        "gpt-oss:7b"
    } else if gpu_memory_gb >= 4.5 {
        "gpt-oss:3b"
    } else {
        "gpt-oss:1b"
    }
}
```

**ルーター側での取得**:
- ノード登録時に `RegisterRequest` から `gpu_memory_total_mb` を取得（現在は送信されていないため、追加が必要）
- または、ハートビートレスポンスの `gpu_memory_total_mb` を使用

**理由**:
- 既存の `MODEL_MEMORY_REQUIREMENTS` を活用
- GPU能力に応じた最適なモデルサイズを自動選択
- ユーザーの手動設定を最小化

**代替案が却下された理由**:
- GPU能力スコア（0-10000）を使う案: メモリサイズの方が直感的で、モデルサイズとの対応が明確
- すべてのノードに同じモデル: GPU能力の無駄、ディスク容量の無駄

---

## まとめ

### 技術スタック確定

| コンポーネント | 技術選択 | 理由 |
|---------------|---------|------|
| モデル一覧取得 | ノード経由 + 事前定義リスト | 公式API不在のため |
| 進捗更新 | Long Polling (5秒間隔) | 既存パターン踏襲、シンプル |
| タスク管理 | tokio::spawn + Arc<Mutex<HashMap>> | シンプル、tokio標準パターン |
| GPU判定 | メモリサイズベース選択 | 既存実装活用、直感的 |

### 要明確化の解決状況

すべての技術的不明点が解決され、Phase 1（設計＆契約）に進む準備が整いました。

---

**Phase 0完了**: 2025-11-12
**次のステップ**: Phase 1 - Design & Contracts
