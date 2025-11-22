# C++ Node Implementation Plan (Complete)

## 概要

Rustノードをllama.cpp統合のC++ノードに置き換える完全実装計画書。Ollamaプロセス依存を排除し、llama.cppを直接統合して高性能なマルチモデルホスティングを実現する。

## 技術選択の根拠

### なぜC++を選んだか
- **Rustからollama.cpp使用の課題**: FFI（bindgen/cxx）でC++をラップすることは可能だが、ビルド・ABI・API変更対応などの保守コストが非常に高い
- **検討した代替案**:
  - `llama-cpp-rs`: RustからGGUFを直接読むバインディング（ollama.cpp不要）
  - `ollama-rs`: Ollama HTTPクライアント（Ollamaサーバー依存）
- **結論**: 「ollama.cppを前提にゴリゴリやるならC++が一番現実的」（GPT-5.1の推奨）

### ollama.cppの制限事項と対策
- **制限**: 単体ではOllama互換HTTPAPIを持っていない → **対策**: cpp-httplibで自前実装
- **制限**: Modelfile的なモデル管理機能は限定的 → **対策**: 簡易版を実装、将来的に拡張
- **制限**: マルチモデル管理や高度なキャッシュ機構なし → **対策**: スレッドプールで独自実装

## 重要な要件

### 必須要件
- **TDD（Test-Driven Development）遵守**: Red-Green-Refactorサイクル厳守
- **GPU必須**: ノード登録にはGPU検出が必須（GPUなしでは動作不可）
- **ルーター互換性**: 既存のRustルーターとの完全な互換性
- **モデル同期**: ルーターの`/v1/models`から自動同期
- **Ollamaモデル互換**: `~/.ollama/models/`の既存モデルを活用
- **自動リリース**: semantic-releaseによる自動バージョニング

### 対応モデル（ルーターから同期）
- gpt-oss:20b
- gpt-oss:120b
- gpt-oss-safeguard:20b
- qwen3-coder:30b

## アーキテクチャ

```
ollama-node-cpp/
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

#### Ollamaモデル互換 (models/) - RED-GREEN-REFACTOR
- [x] **TEST FIRST**: tests/unit/ollama_compat_test.cpp
  - [x] マニフェスト解析テスト
  - [x] GGUFパス解決テスト
  - [x] モデル検証テスト
  - [x] エラー処理テスト
- [x] ollama_compat.h の作成
- [x] ollama_compat.cpp の実装
  - [x] ~/.ollama/models/ のマニフェスト解析
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
  - [x] Ollamaレジストリ（registry.ollama.ai）通信
  - [x] Blobダウンロード（チャンク処理）
  - [x] 進捗報告機能
  - [x] ~/.ollama/models/ への保存
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

### Phase 3: llama.cpp統合（TDD）

#### モデルマネージャー (core/) - RED-GREEN-REFACTOR
- [x] **TEST FIRST**: tests/unit/llama_manager_test.cpp
  - [x] モデルロードテスト
  - [x] コンテキスト作成テスト
  - [x] GPU/CPUレイヤー分割テスト
  - [x] メモリ管理テスト
- [x] llama_manager.h の作成
- [x] llama_manager.cpp の実装
  - [x] llama.cpp初期化（ダミー）
  - [x] GGUFファイルロード
  - [x] コンテキスト管理
  - [x] GPU/CPUレイヤー分割
  - [x] エラーハンドリング
- [x] **REFACTOR**: コードクリーンアップ

#### モデルプール (core/) - RED-GREEN-REFACTOR
- [x] **TEST FIRST**: tests/unit/model_pool_test.cpp
  - [x] 複数モデル管理テスト
  - [x] スレッド安全性テスト
  - [x] 動的ロード/アンロードテスト
  - [x] メモリ制限テスト
- [x] model_pool.h の作成
- [x] model_pool.cpp の実装
  - [x] 複数モデルインスタンス管理（最低限）
  - [x] スレッドごとのモデル割り当て
  - [x] 動的ロード/アンロード
  - [x] メモリ管理とGC
  - [x] ロック機構
- [x] **REFACTOR**: コードクリーンアップ

#### 推論エンジン (core/) - RED-GREEN-REFACTOR
- [x] **TEST FIRST**: tests/unit/inference_engine_test.cpp
  - [x] プロンプト処理テスト
  - [x] トークン生成テスト
  - [x] ストリーミングテスト
  - [x] バッチ推論テスト
- [x] inference_engine.h の作成
- [x] inference_engine.cpp の実装
  - [x] プロンプト処理
  - [x] トークン生成
  - [x] ストリーミング対応
  - [x] バッチ推論
  - [x] サンプリング戦略
- [x] **REFACTOR**: コードクリーンアップ

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
- [ ] **REFACTOR**: コードクリーンアップ

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
- [ ] **REFACTOR**: コードクリーンアップ

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
- [ ] **REFACTOR**: コードクリーンアップ

### Phase 5: 統合とテスト

#### メインプログラム更新
- [ ] **TEST FIRST**: tests/integration/main_test.cpp
- [ ] main.cpp の完全実装
  - [ ] 設定読み込み（環境変数/設定ファイル）
  - [ ] 初期化フロー（順序保証）
  - [ ] グレースフルシャットダウン
  - [ ] エラーリカバリー
  - [ ] シグナルハンドリング

#### ユーティリティ (utils/)
- [ ] **TEST FIRST**: tests/unit/utils_test.cpp
- [x] config.h/cpp - 設定管理（環境変数・JSON・ファイルロック対応）
- [ ] logger.h/cpp - ログシステム（spdlog統合）
- [ ] json_utils.h/cpp - JSON処理ヘルパー
- [ ] system_info.h/cpp - システム情報取得

### Phase 6: CI/CD・自動リリース

#### GitHub Actions設定
- [ ] .github/workflows/ci.yml
  - [ ] ビルドマトリックス（Linux/macOS/Windows）
  - [ ] テスト実行（unit/integration/contract）
  - [ ] コードカバレッジ測定
  - [ ] 静的解析（clang-tidy, cppcheck）

- [ ] .github/workflows/release.yml
  - [ ] semantic-release統合
  - [ ] 自動バージョニング（commitlintベース）
  - [ ] バイナリビルド（各プラットフォーム）
  - [ ] リリースノート自動生成
  - [ ] アセット自動アップロード

#### commitlint設定
- [ ] .commitlintrc.json
- [ ] husky設定（pre-commit）
- [ ] conventional commits準拠

#### semantic-release設定
- [ ] .releaserc.json
  - [ ] バージョニングルール
  - [ ] アセット定義
  - [ ] リリースノート設定

### Phase 7: パッケージング・配布

#### Dockerコンテナ
- [ ] Dockerfile
  - [ ] マルチステージビルド
  - [ ] 最小イメージサイズ
  - [ ] GPU対応（nvidia-docker）

- [ ] docker-compose.yml
  - [ ] ルーターとの連携設定
  - [ ] ボリュームマウント（モデル）
  - [ ] ネットワーク設定

#### パッケージマネージャー
- [ ] Debian/Ubuntuパッケージ（.deb）
- [ ] RedHat/CentOSパッケージ（.rpm）
- [ ] macOS Homebrew Formula
- [ ] Windows MSIインストーラー

#### インストールスクリプト
- [ ] install.sh（Linux/macOS）
- [ ] install.ps1（Windows）
- [ ] 自動アップデート機構

### Phase 8: ドキュメント

#### ユーザードキュメント
- [ ] README.md（日本語/英語）
- [ ] INSTALL.md - インストールガイド
- [ ] USAGE.md - 使用方法
- [ ] TROUBLESHOOTING.md - トラブルシューティング

#### 開発者ドキュメント
- [ ] CONTRIBUTING.md - コントリビューションガイド
- [ ] ARCHITECTURE.md - アーキテクチャ説明
- [ ] API.md - API仕様書
- [ ] DEVELOPMENT.md - 開発環境セットアップ

#### API仕様
- [ ] OpenAPI/Swagger定義
- [ ] Postmanコレクション
- [ ] APIクライアント例（各言語）

### Phase 9: パフォーマンス・最適化

#### ベンチマーク
- [ ] 推論速度ベンチマーク
- [ ] メモリ使用量測定
- [ ] スループット測定
- [ ] レイテンシ測定

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
- [ ] Prometheusエクスポーター
- [ ] Grafanaダッシュボード
- [ ] アラート設定

#### ログ管理
- [ ] 構造化ログ（JSON）
- [ ] ログローテーション
- [ ] ログレベル動的変更
- [ ] 分散トレーシング対応

#### ヘルスチェック
- [ ] リビングプローブ
- [ ] レディネスプローブ
- [ ] スタートアッププローブ

## 優先順位（実装順序）

### 🚨 最優先（TDD必須）
1. テスト環境構築とコントラクトテスト
2. GPU検出（テスト→実装）
3. ルーター登録（テスト→実装）
4. モデル同期（テスト→実装）

### 📍 必須機能（MVP）
1. Ollamaモデル読み込み（テスト→実装）
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

1. ✅ PLANS.md作成（.agent/に配置）
2. ⬜ テスト環境構築（Google Test導入）
3. ⬜ コントラクトテスト作成
4. ⬜ GPU検出のテスト作成
5. ⬜ GPU検出の実装完了

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
