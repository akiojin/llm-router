# Phase 0: リサーチ結果

**機能**: リクエスト/レスポンス履歴保存機能
**日付**: 2025-11-03

## 技術的不明点の解決

### 1. ストリーミングレスポンスのキャプチャ方法

**決定**: `hyper::Body` をバッファリングしてからクライアントに転送

**理由**:
- レスポンス全体を保存する必要があるため、完全なバッファリングが必須
- Axum + Tokio エコシステムで標準的なアプローチ
- `hyper::body::to_bytes()` で簡潔に実装可能

**検討した代替案**:
1. **チャンクごとに保存**
   - メリット: メモリ効率が良い
   - デメリット: 再構築が複雑、ファイルI/Oが頻繁
   - 却下理由: 複雑さがメリットを上回る

2. **T字パイプ（同時に複数ストリームへ）**
   - メリット: クライアントへのレスポンスと保存を並行処理
   - デメリット: Axum + Tokioでの実装が複雑、エラーハンドリングが困難
   - 却下理由: 過剰設計、憲章のシンプルさ原則に反する

**実装パターン**:
```rust
// proxy.rs 内
let response_bytes = hyper::body::to_bytes(response.into_body()).await?;
let response_body: serde_json::Value = serde_json::from_slice(&response_bytes)?;

// 非同期で保存
tokio::spawn(async move {
    save_record(record).await;
});

// クライアントにレスポンス返却
Ok(Response::new(Body::from(response_bytes)))
```

**トレードオフ**:
- メモリ使用量が増加（大きなレスポンス時）
- 但し一時的で、レスポンス返却後は解放
- 7日間で10,000件想定、問題なし

---

### 2. 非同期ファイル保存の実装

**決定**: `tokio::spawn` で別タスクとして保存処理を実行

**理由**:
- プロキシのレスポンス返却を待たせない（パフォーマンス最優先）
- 保存失敗がレスポンスに影響しない（可用性確保）
- Tokioランタイムで自然なパターン

**検討した代替案**:
1. **チャネル経由のワーカータスク**
   - メリット: 保存処理の並行数を制御可能
   - デメリット: 複雑、チャネル管理が必要
   - 却下理由: 過剰設計、スケールの要件（1000 req/s）で十分シンプルに対応可能

2. **同期保存（ブロッキング）**
   - メリット: エラーハンドリングがシンプル
   - デメリット: レスポンスタイムが悪化（5% → 20%以上のオーバーヘッド）
   - 却下理由: パフォーマンス目標に反する

**実装パターン**:
```rust
tokio::spawn(async move {
    if let Err(e) = save_record(&record).await {
        error!("Failed to save request record: {}", e);
    }
});
```

**エラーハンドリング**:
- Fire-and-forget パターン
- 保存失敗はログに記録のみ
- 重要なエラー（ディスク容量不足等）は tracing::error で通知

---

### 3. 7日間のデータクリーンアップ

**決定**: 定期タスク（`tokio::time::interval`）で1時間ごとに実行

**理由**:
- 即座のクリーンアップは不要（ストレージ容量に余裕）
- バッチ処理でファイルI/Oを削減
- 実装がシンプル

**検討した代替案**:
1. **保存時に毎回クリーンアップ**
   - メリット: データ量が常に最小
   - デメリット: 毎回の処理コストが高い、レスポンスタイムに影響
   - 却下理由: パフォーマンス目標に反する

2. **起動時のみクリーンアップ**
   - メリット: 実装が最もシンプル
   - デメリット: 長時間稼働時にファイルが肥大化
   - 却下理由: 7日間の連続稼働想定で不適切

**実装パターン**:
```rust
tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(3600)); // 1時間
    loop {
        interval.tick().await;
        if let Err(e) = cleanup_old_records(Duration::from_days(7)).await {
            error!("Failed to cleanup old records: {}", e);
        }
    }
});
```

**タイミング**:
- サーバー起動時に1回実行
- その後1時間ごとに実行
- Graceful shutdown対応（Cancellation Token）

---

### 4. 大量レコードのフィルタリング実装

**決定**: メモリ内でのイテレータフィルタ + ページネーション

**理由**:
- JSONファイルは全読み込みが必要（構造上）
- データ量は7日間で管理可能（10,000件 × 10KB = 100MB程度）
- Rustのイテレータは高速で効率的
- 実装がシンプル

**検討した代替案**:
1. **SQLiteインデックス**
   - メリット: 高速な検索、大量データに対応
   - デメリット: 憲章違反（JSONファイル必須）、複雑さ増加
   - 却下理由: 憲章に明記された制約

2. **複数ファイル分割（日付ごと等）**
   - メリット: ファイルサイズが小さい、部分読み込み可能
   - デメリット: 読み込みロジックが複雑、範囲検索が困難
   - 却下理由: シンプルさ原則に反する

**実装パターン**:
```rust
fn filter_records(
    records: Vec<RequestResponseRecord>,
    filter: &RecordFilter,
) -> Vec<RequestResponseRecord> {
    records
        .into_iter()
        .filter(|r| filter.matches(r))
        .collect()
}

fn paginate<T>(items: Vec<T>, page: usize, per_page: usize) -> Vec<T> {
    let start = (page - 1) * per_page;
    items.into_iter().skip(start).take(per_page).collect()
}
```

**スケール見積もり**:
- 10,000件 × 平均10KB = 100MB
- メモリ読み込み: < 200ms
- フィルタリング: < 50ms
- 合計: < 300ms（パフォーマンス目標1秒以内を満たす）

---

### 5. CSVエクスポートの実装

**決定**: `csv` クレート使用、メモリ内でCSV生成してレスポンス

**理由**:
- 標準的なアプローチ
- ストリーミング不要（データ量が小）
- 実装がシンプル

**実装パターン**:
```rust
use csv::Writer;

let mut wtr = Writer::from_writer(vec![]);
for record in records {
    wtr.serialize(record)?;
}
let csv_bytes = wtr.into_inner()?;

Response::builder()
    .header("Content-Type", "text/csv")
    .header("Content-Disposition", "attachment; filename=history.csv")
    .body(Body::from(csv_bytes))
```

**CSVフィールド順序**:
1. timestamp
2. request_type
3. model
4. agent_machine_name
5. status
6. duration_ms
7. request_body (JSON文字列)
8. response_body (JSON文字列)

---

## 技術選択のベストプラクティス

### Axumでのファイルダウンロード

**パターン**:
```rust
use axum::{
    response::{Response, IntoResponse},
    http::{header, StatusCode},
};

async fn export_handler() -> impl IntoResponse {
    let data = generate_export_data().await;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"history.json\"",
        )
        .body(data)
        .unwrap()
}
```

**ベストプラクティス**:
- `Content-Disposition: attachment` でダウンロードを強制
- ファイル名に日時を含めると便利（例: `history-2025-11-03.json`）
- Content-Type を正確に設定

---

### Tokioでの定期タスク

**パターン**:
```rust
use tokio::time::{interval, Duration};

tokio::spawn(async move {
    let mut interval = interval(Duration::from_secs(3600));

    loop {
        interval.tick().await;

        // 定期処理
        if let Err(e) = periodic_task().await {
            tracing::error!("Periodic task failed: {}", e);
        }
    }
});
```

**Graceful Shutdown 対応**:
```rust
use tokio_util::sync::CancellationToken;

let token = CancellationToken::new();
let token_clone = token.clone();

tokio::spawn(async move {
    let mut interval = interval(Duration::from_secs(3600));

    loop {
        tokio::select! {
            _ = interval.tick() => {
                periodic_task().await;
            }
            _ = token_clone.cancelled() => {
                tracing::info!("Periodic task cancelled");
                break;
            }
        }
    }
});
```

---

### ファイルロックの実装

**パターン**:
```rust
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct RequestHistoryStorage {
    file_path: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl RequestHistoryStorage {
    pub async fn save_record(&self, record: &RequestResponseRecord) -> Result<()> {
        let _guard = self.lock.lock().await;

        // ファイル読み込み
        let mut records = self.load_records_unlocked().await?;

        // レコード追加
        records.push(record.clone());

        // ファイル書き込み
        self.save_records_unlocked(&records).await?;

        Ok(())
    }
}
```

**ベストプラクティス**:
- `Arc<Mutex<()>>` で排他制御
- `tokio::fs` の非同期ファイルI/O使用
- エラー時はファイルを破損させない（一時ファイル + rename）

---

## 依存関係の追加

**Cargo.toml に追加**:
```toml
[dependencies]
csv = "1.3"         # CSVエクスポート
```

**既存の依存関係を利用**:
- `uuid`: レコードID生成
- `chrono`: タイムスタンプ処理
- `serde`, `serde_json`: シリアライゼーション
- `tokio`: 非同期ランタイム
- `axum`: Web API
- `tracing`: ロギング

---

## まとめ

すべての技術的不明点を解決し、実装方針を確定しました：

1. ストリーミング: バッファリング方式
2. 非同期保存: tokio::spawn (fire-and-forget)
3. クリーンアップ: 1時間ごとの定期タスク
4. フィルタリング: メモリ内イテレータ
5. エクスポート: `csv` クレート

すべての選択は憲章の原則（シンプルさ、パフォーマンス、TDD）に準拠しています。
