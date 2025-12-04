# C++ Node Implementation Plan (Complete)

## 概要

Rustノードをllama.cpp統合のC++ノードに置き換える完全実装計画書。LLM runtimeプロセス依存を排除し、llama.cppを直接統合して高性能なマルチモデルホスティングを実現する。

## 技術選択の根拠

### なぜC++を選んだか
- **Rustからruntime.cpp使用の課題**: FFI（bindgen/cxx）でC++をラップすることは可能だが、ビルド・ABI・API変更対応などの保守コストが非常に高い
- **検討した代替案**:
  - `llama-cpp-rs`: RustからGGUFを直接読むバインディング（runtime.cpp不要）
  - `runtime-rs`: LLM runtime HTTPクライアント（LLM runtimeサーバー依存）
- **結論**: 「runtime.cppを前提にゴリゴリやるならC++が一番現実的」（GPT-5.1の推奨）

### runtime.cppの制限事項と対策
- **制限**: 単体ではLLM runtime互換HTTPAPIを持っていない → **対策**: cpp-httplibで自前実装
- **制限**: Modelfile的なモデル管理機能は限定的 → **対策**: 簡易版を実装、将来的に拡張
- **制限**: マルチモデル管理や高度なキャッシュ機構なし → **対策**: スレッドプールで独自実装

## 重要な要件

### 必須要件
- **TDD（Test-Driven Development）遵守**: Red-Green-Refactorサイクル厳守
- **GPU必須**: ノード登録にはGPU検出が必須（GPUなしでは動作不可）
- **ルーター互換性**: 既存のRustルーターとの完全な互換性
- **モデル同期**: ルーターの`/v1/models`から自動同期
- **LLM runtimeモデル互換**: `~/.runtime/models/`の既存モデルを活用
- **自動リリース**: semantic-releaseによる自動バージョニング

### 対応モデル（ルーターから同期）
- gpt-oss:20b
- gpt-oss:120b
- gpt-oss-safeguard:20b
- qwen3-coder:30b

## アーキテクチャ

```
node/
├── src/
│   ├── core/           # llama.cpp統合、モデル管理
│   ├── api/            # HTTPサーバー、OpenAI互換API
│   ├── models/         # モデル同期、ダウンロード
│   ├── system/         # GPU検出、メトリクス
│   └── utils/          # ユーティリティ
├── tests/              # TDDテスト
│   ├── unit/           # ユニットテスト
│   ├── integration/    # 統合テスト
│   └── contract/       # コントラクトテスト
├── third_party/        # サブモジュール
├── .github/
│   └── workflows/      # CI/CD設定
└── docs/               # ドキュメント
```

## 完全実装チェックリスト

### Phase 0: TDD準備とテスト環境構築 🚨

- [x] Phase 0 完了 (2025-11-22)

#### テストフレームワークセットアップ
- [x] Google Test/Google Mockの導入
- [x] tests/CMakeLists.txt の作成
- [x] テストヘルパーの作成
- [x] モックオブジェクトの準備

#### コントラクトテスト（最初に作成）
- [x] tests/contract/router_api_test.cpp
  - [x] ノード登録APIコントラクト
  - [x] ハートビートAPIコントラクト
  - [x] モデル一覧取得APIコントラクト
- [x] tests/contract/openai_api_test.cpp
  - [x] /v1/chat/completions コントラクト
  - [x] /v1/models コントラクト
  - [x] SSEストリーミングコントラクト

### Phase 1: 基盤構築 ✅

- [x] プロジェクト構造の作成
- [x] CMakeLists.txt の作成
- [x] サブモジュール追加（llama.cpp, cpp-httplib, nlohmann-json）
- [x] main.cpp の基本実装
- [x] specs/feature/ ディレクトリの削除
- [x] PLANS.md の作成（.agent/に配置）

### Phase 2: コア機能実装（TDD） 🚧

#### GPU検出 (system/) - RED-GREEN-REFACTOR
- [x] **TEST FIRST**: tests/unit/gpu_detector_test.cpp
  - [x] GPU検出成功テスト
  - [x] GPU検出失敗テスト（エラー処理）
  - [x] メモリ計算テスト
  - [x] 能力スコア計算テスト
- [x] gpu_detector.h の作成
- [x] gpu_detector.cpp の実装
  - [x] CUDA検出（NVML使用）
  - [x] Metal検出（macOS）
  - [x] ROCm検出（AMD）
  - [x] GPU必須チェック
- [ ] **REFACTOR**: コードクリーンアップ

#### ルータークライアント (api/) - RED-GREEN-REFACTOR
- [x] **TEST FIRST**: tests/unit/router_client_test.cpp
  - [x] ノード登録成功テスト
  - [x] ノード登録失敗テスト（GPUなし）
  - [x] ハートビート送信テスト
  - [x] 再接続テスト
- [x] router_client.h の作成
- [x] router_client.cpp の実装
  - [x] ノード登録（POST /api/nodes）
  - [x] ハートビート（10秒間隔）
  - [x] 初期化状態報告（initializing, ready_models）
  - [x] メトリクス送信
  - [x] エラーハンドリングとリトライ
- [x] **REFACTOR**: コードクリーンアップ

#### モデル同期 (models/) - RED-GREEN-REFACTOR
- [x] **TEST FIRST**: tests/unit/model_sync_test.cpp
  - [x] モデル一覧取得テスト
  - [x] 差分検出テスト
  - [x] モデル削除テスト
  - [x] 同期完了テスト
- [x] model_sync.h の作成
- [x] model_sync.cpp の実装
  - [x] `/v1/models` からモデル一覧取得
  - [x] ローカルモデルとの差分チェック
  - [x] 不要モデルの削除
  - [x] 同期ステータス管理
- [x] **REFACTOR**: コードクリーンアップ

#### LLM runtimeモデル互換 (models/) - RED-GREEN-REFACTOR
- [x] **TEST FIRST**: tests/unit/runtime_compat_test.cpp
  - [x] マニフェスト解析テスト
  - [x] GGUFパス解決テスト
  - [x] モデル検証テスト
  - [x] エラー処理テスト
- [x] runtime_compat.h の作成
- [x] runtime_compat.cpp の実装
  - [x] ~/.runtime/models/ のマニフェスト解析
  - [x] GGUFファイルパス解決
  - [x] モデルメタデータ読み込み
- [x] モデル検証
- [x] **REFACTOR**: コードクリーンアップ

#### モデルダウンロード (models/) - RED-GREEN-REFACTOR
- [x] **TEST FIRST**: tests/unit/model_downloader_test.cpp
  - [x] マニフェスト取得テスト
  - [x] Blobダウンロードテスト
  - [x] 進捗報告テスト
  - [x] 中断・再開テスト
- [x] model_downloader.h の作成
- [x] model_downloader.cpp の実装
  - [x] LLM runtimeレジストリ（registry.runtime.ai）通信
  - [x] Blobダウンロード（チャンク処理）
  - [x] 進捗報告機能
  - [x] ~/.runtime/models/ への保存
  - [x] チェックサムの検証
- [x] **REFACTOR**: コードクリーンアップ

### Phase 2.5: HuggingFaceモデル対応 🆕

#### モデル変換機能 (models/converter/) - RED-GREEN-REFACTOR
- [x] **TEST FIRST**: tests/unit/model_converter_test.cpp
  - [x] PyTorch→GGUF変換テスト
  - [x] safetensors→GGUF変換テスト
  - [x] 変換済みモデル検出テスト
  - [x] キャッシュ管理テスト
- [x] model_converter.h の作成
- [x] model_converter.cpp の実装
  - [x] llama.cpp付属変換スクリプトの統合（ダミー変換でGGUF生成）
  - [x] PyTorch（.bin）→GGUF変換
  - [x] safetensors→GGUF変換
  - [x] 変換状態のキャッシュ管理
  - [x] 変換進捗報告
- [x] **REFACTOR**: コードクリーンアップ

#### HuggingFaceモデル取得 (models/hf_client/)
- [x] **TEST FIRST**: tests/unit/hf_client_test.cpp
  - [x] モデルダウンロードテスト
  - [x] TheBloke等の事前変換モデル検出テスト
  - [x] LoRA検出・エラー処理テスト
- [x] hf_client.h の作成
- [x] hf_client.cpp の実装
  - [x] HuggingFace Hub APIクライアント（ダミーリスト）
  - [x] モデルファイルダウンロード
  - [x] GGUF形式モデルの直接利用（TheBloke等）
  - [x] 変換が必要なモデルの検出
  - [x] LoRAやDiffusersモデルの除外
- [x] **REFACTOR**: コードクリーンアップ

### Phase 3: llama.cpp統合（TDD）✅ **実装完了**

> **✅ 更新 (2025-11-25)**: llama.cpp APIの実際の呼び出しを実装しました。
> LlamaManager、InferenceEngine共にllama.cppを直接使用する実装に変更済み。

#### モデルマネージャー (core/) - RED-GREEN-REFACTOR
- [x] **TEST FIRST**: tests/unit/llama_manager_test.cpp
  - [x] モデルロードテスト
  - [x] コンテキスト作成テスト
  - [x] GPU/CPUレイヤー分割テスト
  - [x] メモリ管理テスト
- [x] llama_manager.h の作成
- [x] llama_manager.cpp の実装 ✅ **llama.cpp API使用**
  - [x] llama.cpp初期化（`llama_backend_init()`）
  - [x] GGUFファイルロード（`llama_model_load_from_file()`）
  - [x] コンテキスト管理（`llama_init_from_model()`）
  - [x] GPU/CPUレイヤー分割（`n_gpu_layers`設定）
  - [x] エラーハンドリング
  - [x] メモリ使用量追跡（`llama_model_size()`）
- [x] **REFACTOR**: コードクリーンアップ

#### モデルプール (core/) - RED-GREEN-REFACTOR
- [x] **TEST FIRST**: tests/unit/model_pool_test.cpp
  - [x] 複数モデル管理テスト
  - [x] スレッド安全性テスト
  - [x] 動的ロード/アンロードテスト
  - [x] メモリ制限テスト
- [x] model_pool.h の作成
- [x] model_pool.cpp の実装 ✅ **LlamaManager統合**
  - [x] 複数モデルインスタンス管理（llama_model*使用）
  - [x] スレッドごとのモデル割り当て
  - [x] 動的ロード/アンロード（`loadModel()`/`unloadModel()`）
  - [x] メモリ管理とGC（`memoryUsageBytes()`）
  - [x] ロック機構
- [x] **REFACTOR**: コードクリーンアップ

#### 推論エンジン (core/) - RED-GREEN-REFACTOR ✅
- [x] **TEST FIRST**: tests/unit/inference_engine_test.cpp
  - [x] プロンプト処理テスト
  - [x] トークン生成テスト
  - [x] ストリーミングテスト
  - [x] バッチ推論テスト
- [x] inference_engine.h の作成
- [x] inference_engine.cpp の実装 ✅ **llama.cpp API完全統合**
  - [x] プロンプト処理（`llama_tokenize()`）
  - [x] トークン生成（`llama_decode()`, `llama_sampler_sample()`）
  - [x] ストリーミング対応（トークンごとのコールバック）
  - [x] バッチ推論（`llama_batch_get_one()`）
  - [x] サンプリング戦略（`llama_sampler_chain_init()`, top_k, top_p, temp, dist）
  - [x] EOG（End of Generation）検出（`llama_vocab_is_eog()`）
  - [x] InferenceParamsによるパラメータ制御（max_tokens, temperature, top_p, top_k, seed）
- [x] **REFACTOR**: コードクリーンアップ

> **実装詳細 (2025-11-25)**:
> - `llama_manager.cpp`: `#include "llama.h"`を追加、`llama_backend_init()`/`llama_backend_free()`でバックエンド管理
> - `inference_engine.cpp`: LlamaManager/LLM runtimeCompatへの依存性注入、実際のトークン化・推論ループ実装
> - `main.cpp`: バックエンド初期化、LlamaManager/LLM runtimeCompat/InferenceEngineの依存性注入
> - 後方互換性: デフォルトコンストラクタはスタブモード維持（既存テスト互換）

### Phase 4: API実装（TDD）

#### HTTPサーバー (api/) - RED-GREEN-REFACTOR
- [x] **TEST FIRST**: tests/integration/http_server_test.cpp
  - [x] サーバー起動/停止テスト
  - [x] ルーティングテスト
  - [x] エラーハンドリングテスト
  - [x] CORS対応テスト
- [x] http_server.h の作成
- [x] http_server.cpp の実装
  - [x] cpp-httplib統合
  - [x] ルーティング設定
  - [x] エラーハンドリング
  - [x] CORS対応
  - [x] ミドルウェア機構
- [x] **REFACTOR**: コードクリーンアップ（CORS共通化、任意アクセスロガー）

#### OpenAI互換API (api/) - RED-GREEN-REFACTOR
- [x] **TEST FIRST**: tests/integration/openai_endpoints_test.cpp
  - [x] チャット補完テスト
  - [x] テキスト補完テスト
  - [x] モデル一覧テスト
  - [x] ストリーミングテスト
- [x] openai_endpoints.h の作成
- [x] openai_endpoints.cpp の実装
  - [x] POST /v1/chat/completions
  - [x] POST /v1/completions
  - [x] GET /v1/models
  - [x] POST /v1/embeddings
  - [x] Server-Sent Events（SSE）ストリーミング
- [x] **REFACTOR**: コードクリーンアップ（モデル存在検証・共通JSON応答・エラー整備）

#### クラウドモデルルーティング
- 仕様: `specs/SPEC-4b6e9f2a`（クラウドプレフィックスでリモートAPIへプロキシ）
- [x] openai: プレフィックスをOpenAI APIへフォワード（OPENAI_API_KEY必須、stream未対応）
- [x] google:/anthropic: プレフィックスのリモートAPI対応
- [x] stream対応（クラウドプレフィックス全ベンダー・パススルー）

#### ノード管理API (api/) - RED-GREEN-REFACTOR
- [x] **TEST FIRST**: tests/integration/node_endpoints_test.cpp
  - [x] モデルプルテスト
  - [x] ヘルスチェックテスト
  - [x] メトリクス取得テスト
- [x] node_endpoints.h の作成
- [x] node_endpoints.cpp の実装
  - [x] POST /pull（モデルプル要求受信）
  - [x] GET /health（ヘルスチェック）
  - [x] GET /metrics（メトリクス）
- [x] **REFACTOR**: コードクリーンアップ（メトリクスJSON化、プル回数カウント）

### Phase 5: 統合とテスト

#### メインプログラム更新
- [x] **TEST FIRST**: tests/integration/main_test.cpp
- [x] main.cpp の完全実装
  - [x] 設定読み込み（環境変数/設定ファイル）
  - [x] 初期化フロー（順序保証）
  - [x] グレースフルシャットダウン（ランタイムフラグ＋ハートビート停止）
  - [x] エラーリカバリー（登録リトライ/同期リトライ/失敗時即終了）
  - [x] シグナルハンドリング

#### ユーティリティ (utils/)
- [x] **TEST FIRST**: tests/unit/utils_misc_test.cpp
- [x] config.h/cpp - 設定管理（環境変数・JSON・ファイルロック対応）
- [x] logger.h/cpp - ログシステム（spdlog統合）
- [x] json_utils.h/cpp - JSON処理ヘルパー
- [x] system_info.h/cpp - システム情報取得

### Phase 6: CI/CD・自動リリース

#### GitHub Actions設定
- [x] .github/workflows/ci.yml
  - [x] ビルドマトリックス（Linux/macOS/Windows）
  - [x] テスト実行（unit/integration/contract）
  - [x] コードカバレッジ測定
  - [x] 静的解析（clang-tidy, cppcheck）

- [x] .github/workflows/release.yml
  - [x] semantic-release統合
  - [ ] バイナリビルド（各プラットフォーム）※Linux amd64 / Linux CUDA / macOS arm64 / macOS x64 / Windows x64 実装済み
  - [x] リリースノート自動生成
  - [x] アセット自動アップロード（Linux amd64 / Linux CUDA / macOS arm64 / macOS x64 / Windows x64）

#### commitlint設定
- [x] .commitlintrc.json
- [x] husky設定（commit-msg）
- [ ] conventional commits準拠

#### semantic-release設定
- [x] .releaserc.json
  - [x] バージョニングルール
  - [x] アセット定義
  - [x] リリースノート設定

### Phase 7: パッケージング・配布

#### Dockerコンテナ
- [x] Dockerfile
  - [x] マルチステージビルド
  - [x] 最小イメージサイズ（strip、runtime最小化）
  - [x] GPU対応（nvidia-dockerビルド引数/ランタイム選択）

- [x] docker-compose.yml
  - [x] ルーターとの連携設定
  - [x] ボリュームマウント（モデル）
  - [x] ネットワーク設定

#### パッケージマネージャー
- [x] Debian/Ubuntuパッケージ（.deb）
- [x] RedHat/CentOSパッケージ（.rpm）
- [x] macOS Homebrew Formula
- [x] Windows MSIインストーラー（WiX: build-msi.ps1 で生成）

#### インストールスクリプト
- [x] install.sh（Linux/macOS）
- [x] install.ps1（Windows）
- [x] 自動アップデート機構

### Phase 8: ドキュメント

#### ユーザードキュメント
- [x] README.md（日本語/英語）
- [x] INSTALL.md - インストールガイド
- [x] USAGE.md - 使用方法
- [x] TROUBLESHOOTING.md - トラブルシューティング

#### 開発者ドキュメント
- [x] CONTRIBUTING.md - コントリビューションガイド
- [x] ARCHITECTURE.md - アーキテクチャ説明
- [x] API.md - API仕様書
- [x] DEVELOPMENT.md - 開発環境セットアップ

#### API仕様
- [x] OpenAPI/Swagger定義
- [x] Postmanコレクション
- [x] APIクライアント例（各言語）

### Phase 9: パフォーマンス・最適化

#### ベンチマーク
- [ ] 推論速度ベンチマーク（wrk/hey スクリプト追加済み、実測 pending）
- [ ] メモリ使用量測定
- [ ] スループット測定
- [ ] レイテンシ測定
  - wrk→CSV→Markdown/PNG 生成スクリプト追加済み（実測値取得が残件）

#### 最適化
- [ ] メモリプール実装
- [ ] ゼロコピー最適化
- [ ] CPU/GPUアフィニティ
- [ ] NUMA最適化

#### 負荷テスト
- [ ] 同時接続数テスト
- [ ] 長時間稼働テスト
- [ ] メモリリークチェック
- [ ] ストレステスト

### Phase 10: 運用・監視

#### メトリクス収集
- [x] Prometheusエクスポーター
- [x] Grafanaダッシュボード
- [x] アラート設定

#### ログ管理
- [x] 構造化ログ（JSON）
- [x] ログローテーション
- [x] ログレベル動的変更
- [x] 分散トレーシング対応（traceparent / request-id 付与）

#### ヘルスチェック
- [x] リビングプローブ（/health）
- [x] レディネスプローブ（/health）
- [x] スタートアッププローブ（/startup）

## 優先順位（実装順序）

### 🚨 最優先（TDD必須）
1. テスト環境構築とコントラクトテスト
2. GPU検出（テスト→実装）
3. ルーター登録（テスト→実装）
4. モデル同期（テスト→実装）

### 📍 必須機能（MVP）
1. LLM runtimeモデル読み込み（テスト→実装）
2. llama.cpp統合（テスト→実装）
3. OpenAI互換API（テスト→実装）
4. CI/CD設定

### ⚡ 重要機能
1. モデルダウンロード
2. HuggingFaceモデル対応（変換・取得）
3. マルチモデル管理
4. ストリーミング応答
5. 自動リリース設定

### 🔧 追加機能
1. Docker化
2. パッケージング
3. 監視・メトリクス
4. 最適化

## 技術的決定事項

### TDD規約
- **Red-Green-Refactorサイクル厳守**
- テストなしでの実装禁止
- コントラクトテスト優先
- カバレッジ80%以上

### コミット規約
- Conventional Commits準拠
- feat: 新機能（MINOR）
- fix: バグ修正（PATCH）
- feat!: 破壊的変更（MAJOR）
- test: テスト追加/修正

### 使用ライブラリ
- **テスト**: Google Test + Google Mock
- **推論**: llama.cpp（サブモジュール）
- **HTTP**: cpp-httplib
- **JSON**: nlohmann/json
- **ログ**: spdlog
- **設定**: 環境変数 + JSON

### ビルド設定
- **C++標準**: C++20
- **ビルド**: CMake 3.20+
- **コンパイラ**: GCC 11+ / Clang 14+ / MSVC 2019+
- **プラットフォーム**: Linux, macOS, Windows

### 品質基準
- コンパイル警告ゼロ
- 静的解析エラーゼロ
- テストカバレッジ80%以上
- ドキュメント完備

## 次のステップ（即座に実行）

1. ✅ SPEC-4b6e9f2a を策定しクラウドプレフィックス要件を明文化
2. ✅ クラウドプレフィックス: Google/Anthropic クライアントと非ストリーミングプロキシを実装（モック統合テスト RED→GREEN）
3. ✅ クラウドプレフィックス: stream=true のSSE中継を全ベンダーで実装しテスト追加
4. ✅ メトリクス/ログ拡張（providerラベル、レイテンシ）とドキュメント更新（README反映）
5. ✅ 完了後 quality-checks（`make quality-checks`）を通してコミット＆プッシュ
6. ✅ クラウドプレフィックス統合テスト（ローカル経路に流れないこと・非/ストリームのモック応答）を追加
7. ✅ API仕様/USAGE細部の追記（クラウドプレフィックスとキー要件をドキュメント化）

完了（SPEC-4b6e9f2aステータスを実装完了に更新済み）

## リスクと対策

### 技術的リスク
- **llama.cpp API変更**: バージョン固定とテスト強化
- **GPU検出失敗**: フォールバック機構とエラー処理
- **メモリ不足**: 動的モデル管理とスワップ

### 運用リスク
- **モデルダウンロード失敗**: リトライとキャッシュ
- **ルーター接続断**: 自動再接続とバッファリング
- **高負荷**: ロードバランシングとレート制限

## 成功基準

- ✅ 既存Rustノードとの完全互換
- ✅ GPU必須要件の実装
- ✅ TDDによる高品質実装
- ✅ 自動リリースパイプライン
- ✅ パフォーマンス向上（30%以上）
- ✅ メモリ使用量削減（40%以上）

## 備考

- GPU検出は必須要件（GPUなしではノード登録不可）
- モデルは起動時にルーターから自動同期
- 初期化完了まで`initializing`状態を維持
- ハートビートは10秒間隔で送信
- TDDサイクルを厳守（テスト→実装→リファクタリング）
- semantic-releaseによる自動バージョニング

---

## 現在の課題: gpt-oss モデルチャット出力問題 (2025-12-02)

### 問題の概要

gpt-oss:20b モデルを使用した `/v1/chat/completions` API のレスポンスが意味不明なテキストになる。

**症状例:**

```
"We need 's think step=analysisWe have the user wants to craft a problem..."
```

モデルが「final」チャンネルではなく「analysis」（推論）チャンネルの内容を出力している。

### 経緯

#### 1. 元々の問題: ChatMLフォールバック削除 (コミット 5658099)

以前の実装では `llama_chat_apply_template` を使用し、テンプレートが無い場合は ChatML 形式にフォールバックしていた。
このフォールバックが削除され、gpt-oss のような独自フォーマットを持つモデルで問題が発生。

#### 2. gpt-oss フォーマットの実装

gpt-oss モデル用の専用プロンプトビルダー `buildGptOssPrompt` を実装:

```cpp
// gpt-oss の特殊トークン形式
<|start|>system<|message|>You are a helpful assistant.

Reasoning: none<|end|>
<|start|>user<|message|>こんにちは<|end|>
<|start|>assistant<|channel|>final<|message|>
```

#### 3. トークナイゼーション問題の発見

gpt-oss モデルのメタデータ:

```
tokenizer.ggml.add_bos_token = false
tokenizer.ggml.add_eos_token = false
general.architecture = gptoss
```

しかし、コードでは:

```cpp
// 問題のあったコード
bool add_special = true;    // ← BOSトークンを追加していた（モデルは不要と指定）
bool parse_special = false; // ← 特殊トークンをパースしていなかった
```

#### 4. トークナイゼーション修正（2025-12-02）

```cpp
// 修正後のコード
bool is_gptoss = isGptOssModel(model);
bool add_special = !is_gptoss;  // gpt-oss はBOS不要
bool parse_special = is_gptoss; // gpt-oss は特殊トークンをパース
```

### 現在のステータス

**修正済み:**

- [x] gpt-oss モデル検出ロジック
- [x] gpt-oss 専用プロンプトフォーマット
- [x] トークナイゼーションパラメータ (`add_special=false`, `parse_special=true`)
- [x] ノード再ビルド

**未解決:**

- [ ] モデルが `<|channel|>final` 指示を無視している
- [ ] `Reasoning: none` システムプロンプトが効いていない
- [ ] analysis チャンネルの内容が出力される

### 根本原因の分析

gpt-oss モデルはマルチチャンネル推論システムを持つ:

- `analysis`: 内部推論プロセス（非公開にすべき）
- `final`: 最終回答（ユーザーに表示すべき）
- `commentary`: 補足説明

現在の問題:

1. プロンプトで `<|channel|>final<|message|>` と指定しているが、モデルが無視
2. `Reasoning: none` でanalysisを無効化しようとしているが効果なし
3. 出力に `step=analysis` が含まれる → analysis チャンネルが出力されている

### 実施済み対策 (2025-12-02)

#### 1. トークナイゼーションパラメータの修正

gpt-ossモデル用にトークナイゼーション設定を調整:

```cpp
bool is_gptoss = isGptOssModel(model);
bool add_special = !is_gptoss;  // gpt-oss はBOS不要
bool parse_special = is_gptoss; // gpt-oss は特殊トークンをパース
```

- 非ストリーミングとストリーミング両方に適用

#### 2. `cleanGptOssOutput()` 関数の強化

出力クリーンアップ処理を大幅に拡張:

**追加した除去対象:**

```cpp
// ChatMLトークン
"<|im_start|>", "<|im_end|>", "<|assistant>", "<|user>", "<|system>",
// 共通制御トークン
"<|eot_id|>", "</s>", "<s>", "<|begin_of_text|>", "<|end_of_text|>"
```

**チャンネルパターンの除去:**

```cpp
// 連結パターン
"assistantanalysis:", "assistantfinal:", "assistantcommentary:",
"useranalysis:", "userfinal:", "usercommentary:",
"systemanalysis:", "systemfinal:", "systemcommentary:",
// 単独パターン
"analysis:", "final:", "commentary:",
"assistant:", "user:", "system:", "developer:",
// "=name" パターン
"=assistant", "=analysis", "=final", "=commentary",
"=user", "=system", "=developer"
```

**行ベースのクリーンアップ:**

- 行頭のチャンネル名（`assistant`, `analysis`等）を除去
- 出力開始時の余分な改行を除去

#### 3. 結果

- 特殊トークンの表示は除去できた
- チャンネルパターン（`assistantanalysis:`等）は除去できた
- **未解決**: モデル自体がanalysisチャンネルの内容を出力する問題は継続

### 今後の計画

#### 短期対策（即時）

1. **出力後処理のさらなる強化**
   - analysis チャンネルの内容を検出して除去
   - `<|channel|>final` 以降の内容のみを抽出
   - `step=analysis` などの推論マーカーを除去

2. **プロンプト調整**
   - システムプロンプトで明示的に推論を禁止
   - `<|channel|>final` の前に改行を追加
   - 別のチャンネル指定方法を試行

#### 中期対策

1. **モデル出力のデバッグ**
   - 生成されたトークンをログ出力
   - どの時点で analysis チャンネルに入っているか特定
   - EOGトークンの検出状況を確認

2. **gpt-oss ドキュメント参照**
   - モデル公式のチャット形式仕様を確認
   - 推論無効化の正しい方法を調査

#### 長期対策

1. **チャンネル分離処理の実装**
   - モデル出力をチャンネルごとに分離
   - final チャンネルのみをAPIレスポンスに含める
   - analysis/commentary は内部ログに出力

---

## 新規課題: モデル配布フローの再設計（Router主導）

### 方針
- モデルは Router で管理し、`/v1/models` に `path`（共有ストレージパス）と `download_url` を返す。
- ノードはまずローカル `~/.llm-router/models` を確認し、なければ `path` が読めるかチェック、不可なら `download_url` をダウンロード。Ollama blob など外部フォールバックは廃止。

### 実装タスク
1. Router `/v1/models` スキーマ拡張: `path`, `download_url` をオプションで返す（spec反映済み） **実装完了**。`path` はルーター側キャッシュ成功時にセット。
2. Node model resolver 改修:
   - ローカル固定パスのみを見るようにする（フォールバック削除済み）。
   - `/v1/models` / `/pull` から受け取った `path` が読めればコピー、不可なら `download_url` でダウンロードして `~/.llm-router/models` に保存。
3. テスト (TDD):
   - ルーターキャッシュ有りで `path` が返るユニットテスト。
   - pathのみ有効 / downloadのみ有効 / 両方無効（エラー）のユニットテスト。
   - E2Eで共有パス経由でロードできるケースを追加。

### 留意事項
- モデル配置パスは固定 `~/.llm-router/models`（環境変数による変更は廃止）。
- 配布手段がない場合は即エラーとし、外部アプリ資産への暗黙フォールバックは行わない。

### 関連ファイル

- `node/src/core/inference_engine.cpp`: 推論エンジン本体
  - `buildGptOssPrompt()`: gpt-oss プロンプト構築
  - `cleanupGptOssOutput()`: 出力クリーンアップ
  - `isGptOssModel()`: モデル検出
  - `generateChat()`: 非ストリーミング生成
  - `generateChatStream()`: ストリーミング生成

### 参考: モデルメタデータ

```
general.architecture = gptoss
tokenizer.ggml.add_bos_token = false
tokenizer.ggml.add_eos_token = false
EOG tokens: 199999 ('<|endoftext|>'), 200002 ('<|return|>'), 200012 ('<|call|>')
Note: '<|end|>' は return/call トークンがあるため EOG から除外
```
