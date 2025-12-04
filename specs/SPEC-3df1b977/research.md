# 技術リサーチ: モデルファイル破損時の自動修復機能

**機能ID**: `SPEC-3df1b977`
**日付**: 2025-11-27

## 1. モデルファイル破損の検出方法

### 決定: llama.cppロード失敗 + ファイルサイズ/ヘッダー検証

### 理由

- llama.cppの`llama_model_load_from_file()`は破損ファイルで`nullptr`を返す
- GGUFファイルは固定ヘッダー（マジックナンバー）を持つため、最小限の検証が可能
- 完全なSHA256検証は既存の`LLM runtimeCompat::validateModel()`で利用可能

### 検討した代替案

1. **事前SHA256検証**: すべてのロード前にチェックサムを計算
   - 却下理由: パフォーマンスオーバーヘッドが大きい（数GBのファイル）
2. **ファイル存在チェックのみ**: ファイルが存在すればOK
   - 却下理由: 部分的破損を検出できない

## 2. 既存インフラの活用

### 決定: ModelDownloader + ModelSyncを再利用

### 理由

既存コンポーネントが以下を提供:

- `ModelDownloader::downloadBlob()`: リトライ、帯域制御、SHA256検証
- `ModelSync::downloadWithHint()`: ETagキャッシュ、サイズヒント
- `LLM runtimeCompat::resolveGguf()`: モデル名→パス解決

### 既存コードの修正箇所

| ファイル | 変更内容 |
|---------|---------|
| `llama_manager.cpp` | エラー種別の詳細化（bool → enum） |
| `inference_engine.cpp` | 自動修復フローの追加 |
| `openai_endpoints.cpp` | HTTPステータスコードの適切な返却 |

## 3. 同時修復リクエストの重複防止

### 決定: std::condition_variableによる待機

### 理由

- シンプルで標準C++のみで実装可能
- 既存のmutexパターンと整合
- 修復完了を待機するリクエストに通知可能

### 実装パターン

```cpp
// 疑似コード
std::mutex repair_mutex_;
std::condition_variable repair_cv_;
std::unordered_map<std::string, bool> repairing_models_;

bool waitForRepair(const std::string& model_path, std::chrono::milliseconds timeout) {
    std::unique_lock<std::mutex> lock(repair_mutex_);
    return repair_cv_.wait_for(lock, timeout, [&] {
        return repairing_models_.find(model_path) == repairing_models_.end();
    });
}
```

## 4. エラーメッセージの設計

### 決定: 原因別の構造化エラー

### エラー種別

| エラーコード | 説明 | HTTPステータス |
|-------------|------|---------------|
| `model_corrupted` | ファイル破損、修復試行中 | 503 |
| `repair_failed` | 修復失敗（ネットワーク） | 503 |
| `storage_full` | ストレージ容量不足 | 507 |
| `model_not_found` | モデルが存在しない | 404 |
| `repair_timeout` | 修復タイムアウト | 504 |

## 5. タイムアウト設計

### 決定: 5分（300秒）デフォルト、環境変数で設定可能

### 理由

- 仕様の成功基準: 「10GBまでのモデルを5分以内」
- 大規模モデル（20B）は約15GB、余裕を持たせる
- `LLM_REPAIR_TIMEOUT_SECS`で運用時に調整可能

## 6. ロギング設計

### 決定: spdlogの既存パターンを踏襲

### ログレベル

| イベント | レベル | 例 |
|---------|-------|-----|
| 修復開始 | INFO | `Starting auto-repair for model: gpt-oss:7b` |
| 進捗更新 | DEBUG | `Download progress: 50% (5GB/10GB)` |
| 修復成功 | INFO | `Auto-repair completed: gpt-oss:7b` |
| 修復失敗 | ERROR | `Auto-repair failed: network error` |

## 7. 既存テストの分析

### 現状

- `llama_manager_test.cpp`: モデルロード失敗テストあり
- `model_sync_test.cpp`: ダウンロード・同期テストあり
- **ギャップ**: 自動修復フローのテストなし

### 追加テスト計画

1. 破損モデル検出テスト
2. 自動修復成功テスト
3. 修復失敗時のエラーメッセージテスト
4. 同時修復リクエストの重複防止テスト
5. タイムアウトテスト
