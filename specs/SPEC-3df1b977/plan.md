# 実装計画: モデルファイル破損時の自動修復機能

**機能ID**: `SPEC-3df1b977` | **日付**: 2025-11-27 | **仕様**: [spec.md](spec.md)
**入力**: `/specs/SPEC-3df1b977/spec.md`の機能仕様

## 実行フロー (/speckit.plan コマンドのスコープ)

```
1. 入力パスから機能仕様を読み込み ✅
2. 技術コンテキストを記入 ✅
3. 憲章チェックセクションを評価 ✅
4. Phase 0 を実行 → research.md ✅
5. Phase 1 を実行 → contracts, data-model.md, quickstart.md ✅
6. 憲章チェックセクションを再評価 ✅
7. Phase 2 を計画 → タスク生成アプローチを記述 ✅
8. 停止 - /speckit.tasks コマンドの準備完了
```

## 概要

モデルファイルが破損している場合にシステムが自動的に再ダウンロードし、
ユーザーの操作なしでリクエストを処理する機能を実装する。

主要要件:

- FR-001: モデル読み込み時の破損検出
- FR-002: 自動再ダウンロード開始
- FR-003: 再ロード試行
- FR-004: 具体的エラーメッセージ
- FR-005: 修復中リクエストの待機
- FR-006: 重複修復防止
- FR-007: 進捗ログ記録

## 技術コンテキスト

**言語/バージョン**: C++17
**主要依存関係**: llama.cpp, httplib, spdlog, nlohmann/json
**ストレージ**: ファイルシステム（LLM runtime形式）
**テスト**: Google Test (gtest)
**対象プラットフォーム**: Linux (Docker), macOS
**プロジェクトタイプ**: single
**パフォーマンス目標**: 10GBモデルを5分以内に修復
**制約**: 修復中のリクエストは300秒でタイムアウト
**スケール/スコープ**: 単一ノード、同時リクエスト数10程度

## 憲章チェック

*ゲート: Phase 0 research前に合格必須。Phase 1 design後に再チェック。*

**シンプルさ**:

- プロジェクト数: 1 (node) ✅
- フレームワークを直接使用? ✅ (httplib, spdlogを直接使用)
- 単一データモデル? ✅ (既存のLlamaContext拡張のみ)
- パターン回避? ✅ (Repository/UoWなし、直接実装)

**アーキテクチャ**:

- すべての機能をライブラリとして? ✅ (node/src/core, node/src/models)
- ライブラリリスト:
  - `core/llama_manager`: モデル管理・ロード
  - `core/inference_engine`: 推論実行
  - `models/model_sync`: モデル同期・ダウンロード
  - `models/runtime_compat`: LLM runtime互換レイヤー
- ライブラリごとのCLI: N/A (組み込みライブラリ)
- ライブラリドキュメント: quickstart.md作成済み

**テスト (妥協不可)**:

- RED-GREEN-Refactorサイクルを強制? ✅
- Gitコミットはテストが実装より先に表示? ✅ (計画)
- 順序: Contract→Integration→E2E→Unitを厳密に遵守? ✅
- 実依存関係を使用? ✅ (実際のファイルシステム、スタブHTTPサーバー)
- Integration testの対象: 新機能の契約変更 ✅
- 禁止: テスト前の実装、REDフェーズのスキップ ✅

**可観測性**:

- 構造化ロギング含む? ✅ (spdlog使用)
- エラーコンテキスト十分? ✅ (エラー種別・詳細メッセージ)

**バージョニング**:

- semantic-releaseで自動管理 ✅

## プロジェクト構造

### ドキュメント (この機能)

```
specs/SPEC-3df1b977/
├── spec.md              # 機能仕様 (作成済み)
├── plan.md              # このファイル
├── research.md          # Phase 0 出力 (作成済み)
├── data-model.md        # Phase 1 出力 (作成済み)
├── quickstart.md        # Phase 1 出力 (作成済み)
└── tasks.md             # Phase 2 出力 (/speckit.tasksで作成)
```

### ソースコード (リポジトリルート)

```
node/
├── include/
│   ├── core/
│   │   ├── llama_manager.h      # 修正: ModelLoadError enum追加
│   │   └── inference_engine.h   # 修正: 修復機能の依存注入
│   └── models/
│       ├── model_sync.h         # 既存
│       └── model_repair.h       # 新規: 修復コーディネーター
├── src/
│   ├── core/
│   │   ├── llama_manager.cpp    # 修正: loadModelWithRepair追加
│   │   └── inference_engine.cpp # 修正: 自動修復フロー追加
│   ├── models/
│   │   └── model_repair.cpp     # 新規: 修復ロジック
│   └── api/
│       └── openai_endpoints.cpp # 修正: HTTPステータス適切化
└── tests/
    ├── unit/
    │   ├── llama_manager_test.cpp  # 修正: 破損検出テスト追加
    │   └── model_repair_test.cpp   # 新規: 修復ロジックテスト
    └── integration/
        └── auto_repair_test.cpp    # 新規: E2E修復テスト
```

**構造決定**: オプション1（単一プロジェクト）を使用

## Phase 0: アウトライン＆リサーチ

✅ 完了 - [research.md](research.md) 参照

主要決定:

1. llama.cppロード失敗 + ファイルサイズ検証で破損検出
2. 既存のModelDownloader + ModelSyncを再利用
3. std::condition_variableで同時修復の待機を実装
4. 5分（300秒）デフォルトタイムアウト

## Phase 1: 設計＆契約

✅ 完了

**出力**:

- [data-model.md](data-model.md) - エンティティ定義
- [quickstart.md](quickstart.md) - 使用方法

### 契約定義

#### 1. ModelLoadError列挙型

```cpp
// node/include/core/llama_manager.h
enum class ModelLoadError {
    None,           // 成功
    FileNotFound,   // ファイルなし
    InvalidFormat,  // 拡張子エラー
    Corrupted,      // 破損
    ContextFailed,  // コンテキスト作成失敗
    Unknown
};
```

#### 2. LlamaManager拡張

```cpp
// node/include/core/llama_manager.h
class LlamaManager {
public:
    // 既存メソッドはそのまま

    // 新規: 修復付きロード
    std::pair<bool, ModelLoadError> loadModelWithRepair(
        const std::string& model_path,
        const std::string& model_name,
        std::function<bool(const std::string&)> repair_fn);

    // 新規: 設定
    void setAutoRepair(bool enabled);
    void setRepairTimeout(std::chrono::milliseconds timeout);

    // 新規: 状態確認
    bool isRepairing(const std::string& model_path) const;
    bool waitForRepair(const std::string& model_path,
                       std::chrono::milliseconds timeout);
};
```

#### 3. ModelRepair (新規クラス)

```cpp
// node/include/models/model_repair.h
class ModelRepair {
public:
    ModelRepair(ModelSync& sync, ModelDownloader& downloader,
                LLM runtimeCompat& compat);

    // モデルを修復（再ダウンロード）
    RepairResult repair(const std::string& model_name,
                        std::chrono::milliseconds timeout);

    // 修復が必要か判定
    bool needsRepair(const std::string& model_path) const;
};
```

## Phase 2: タスク計画アプローチ

*このセクションは/speckit.tasksコマンドが実行することを記述*

**タスク生成戦略**:

- Phase 1設計ドキュメントからタスクを生成
- TDD順序: テストが実装より先

**順序戦略**:

1. **Setup** (依存関係)
   - 新規ファイル作成（ヘッダー、空実装）

2. **Test (RED)** - 失敗するテストを先に書く
   - ModelLoadErrorテスト
   - 破損検出テスト
   - 修復成功テスト
   - 重複修復防止テスト
   - タイムアウトテスト

3. **Core (GREEN)** - テストを通す実装
   - ModelLoadError enum実装
   - LlamaManager拡張
   - ModelRepairクラス実装

4. **Integration**
   - InferenceEngine修復フロー統合
   - OpenAI API HTTPステータス修正

5. **Polish**
   - ログメッセージ改善
   - ドキュメント更新

**推定出力**: tasks.mdに20-25個の番号付き、順序付きタスク

**重要**: このフェーズは/speckit.tasksコマンドで実行、/speckit.planではない

## Phase 3+: 今後の実装

*これらのフェーズは/planコマンドのスコープ外*

**Phase 3**: タスク実行 (/speckit.tasksコマンドがtasks.mdを作成)
**Phase 4**: 実装 (憲章原則に従ってtasks.mdを実行)
**Phase 5**: 検証 (テスト実行、quickstart.md実行、パフォーマンス検証)

## 複雑さトラッキング

*憲章チェックに正当化が必要な違反がある場合のみ記入*

| 違反 | 必要な理由 | より単純な代替案が却下された理由 |
|------|-----------|--------------------------------|
| なし | - | - |

## 進捗トラッキング

*このチェックリストは実行フロー中に更新される*

**フェーズステータス**:

- [x] Phase 0: Research完了 (/speckit.plan コマンド)
- [x] Phase 1: Design完了 (/speckit.plan コマンド)
- [x] Phase 2: Task planning完了 (/speckit.plan コマンド - アプローチのみ記述)
- [x] Phase 3: Tasks生成済み (/speckit.tasks コマンド)
- [ ] Phase 4: 実装完了
- [ ] Phase 5: 検証合格

**ゲートステータス**:

- [x] 初期憲章チェック: 合格
- [x] 設計後憲章チェック: 合格
- [x] すべての要明確化解決済み
- [x] 複雑さの逸脱を文書化済み

---
*憲章 v2.1.1 に基づく - `/memory/constitution.md` 参照*
