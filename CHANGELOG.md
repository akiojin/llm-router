# [2.1.0](https://github.com/akiojin/ollama-router/compare/v2.0.5...v2.1.0) (2025-11-19)


### Features

* **installer:** ルーターとノードのインストーラーを分離 ([1a29f9f](https://github.com/akiojin/ollama-router/commit/1a29f9fd732a6867931387211760a28c7dd34096))

## [2.1.1](https://github.com/akiojin/ollama-router/compare/v2.1.0...v2.1.1) (2025-11-20)


### Bug Fixes

* **ci:** align release-please inputs ([94e5b91](https://github.com/akiojin/ollama-router/commit/94e5b91a4a231df207c3146bc8ee63b959201087))
* **ci:** finalize release-please setup on main ([523cd38](https://github.com/akiojin/ollama-router/commit/523cd384c7ff4d37c7bb0f8d00aa7e69e74f616e))
* **ci:** remove stale needs from publish backmerge ([58a04f9](https://github.com/akiojin/ollama-router/commit/58a04f9535da4cac57bde8272bc1723518ddd734))
* **ci:** use release-please action manifest on main ([bce65ed](https://github.com/akiojin/ollama-router/commit/bce65ed266896bd3a672930f91f118c4522d6381))

## [2.0.5](https://github.com/akiojin/ollama-router/compare/v2.0.4...v2.0.5) (2025-11-19)


### Bug Fixes

* **ci:** extract release version from commit message in publish workflow ([6bb1165](https://github.com/akiojin/ollama-router/commit/6bb11657d8a70b323819307fb53a10d0ad02e451))
* **ci:** skip ICE03 validation for Windows MSI build ([3391b7d](https://github.com/akiojin/ollama-router/commit/3391b7d0c120e249837ddd985c1da13d0dba9b15))
* **deps:** upgrade sqlx to 0.8.6 to address security vulnerability ([cef2542](https://github.com/akiojin/ollama-router/commit/cef2542b08144154a02546f29d763c9ef508c026)), closes [#3](https://github.com/akiojin/ollama-router/issues/3)
* **docs:** ワークフローファイル名を実際のファイル名に修正 ([b71d128](https://github.com/akiojin/ollama-router/commit/b71d1281653533a61c1679c3ab76a94d5c36830f))
* **installer:** move KeyPath from RemoveFolder to RegistryValue ([d23230e](https://github.com/akiojin/ollama-router/commit/d23230e5c2756be7f4feed3c17d537f57e97f220))
* **installer:** resolve ICE03 validation errors in WiX ([1303301](https://github.com/akiojin/ollama-router/commit/1303301196269fb3b3375feee443787f4b52e150))
* **release:** improve tag detection in release workflow ([c317474](https://github.com/akiojin/ollama-router/commit/c317474f8aed2b8d149559e0c8dd315cac28da92))
* **release:** improve workflow robustness with tag-based verification ([6f18acb](https://github.com/akiojin/ollama-router/commit/6f18acb7d19506cfa08aa7747b17861c81c3410a))
* **release:** resolve git fetch error in verify step ([7d3ce0c](https://github.com/akiojin/ollama-router/commit/7d3ce0c742f9cabb36c757dd7e68b8297f2c50fc))

## [2.0.5](https://github.com/akiojin/ollama-router/compare/v2.0.4...v2.0.5) (2025-11-19)


### Bug Fixes

* **ci:** extract release version from commit message in publish workflow ([6bb1165](https://github.com/akiojin/ollama-router/commit/6bb11657d8a70b323819307fb53a10d0ad02e451))
* **ci:** skip ICE03 validation for Windows MSI build ([3391b7d](https://github.com/akiojin/ollama-router/commit/3391b7d0c120e249837ddd985c1da13d0dba9b15))
* **deps:** upgrade sqlx to 0.8.6 to address security vulnerability ([cef2542](https://github.com/akiojin/ollama-router/commit/cef2542b08144154a02546f29d763c9ef508c026)), closes [#3](https://github.com/akiojin/ollama-router/issues/3)
* **installer:** move KeyPath from RemoveFolder to RegistryValue ([d23230e](https://github.com/akiojin/ollama-router/commit/d23230e5c2756be7f4feed3c17d537f57e97f220))
* **installer:** resolve ICE03 validation errors in WiX ([1303301](https://github.com/akiojin/ollama-router/commit/1303301196269fb3b3375feee443787f4b52e150))
* **release:** improve tag detection in release workflow ([c317474](https://github.com/akiojin/ollama-router/commit/c317474f8aed2b8d149559e0c8dd315cac28da92))
* **release:** improve workflow robustness with tag-based verification ([6f18acb](https://github.com/akiojin/ollama-router/commit/6f18acb7d19506cfa08aa7747b17861c81c3410a))
* **release:** resolve git fetch error in verify step ([7d3ce0c](https://github.com/akiojin/ollama-router/commit/7d3ce0c742f9cabb36c757dd7e68b8297f2c50fc))

## [2.0.4](https://github.com/akiojin/ollama-router/compare/v2.0.3...v2.0.4) (2025-11-19)


### Bug Fixes

* **build:** resolve wix per-user shortcut registry issues ([5ba4f71](https://github.com/akiojin/ollama-router/commit/5ba4f71222d0e2ae11ae0750f7270df01a1c1a54))

## [2.0.3](https://github.com/akiojin/ollama-router/compare/v2.0.2...v2.0.3) (2025-11-18)


### Bug Fixes

* **build:** set wix codepage to utf8 and shorten icon ids ([0e7a3af](https://github.com/akiojin/ollama-router/commit/0e7a3af0d459dad03b708a78328df2499180a9b5))

## [2.0.2](https://github.com/akiojin/ollama-router/compare/v2.0.1...v2.0.2) (2025-11-18)


### Bug Fixes

* **ci:** update mac runners and wix asset paths ([63f8107](https://github.com/akiojin/ollama-router/commit/63f8107d0d58bc771db48a120f54850597c3828e))

## [2.0.1](https://github.com/akiojin/ollama-router/compare/v2.0.0...v2.0.1) (2025-11-18)


### Bug Fixes

* **installer:** sync windows msi binary names ([785494c](https://github.com/akiojin/ollama-router/commit/785494ce6f2f30586a651cc222fb2e568ae5e907))

# [2.0.0](https://github.com/akiojin/ollama-router/compare/v1.5.0...v2.0.0) (2025-11-18)


* feat!: プロジェクト名をollama-routerに変更し、用語をrouter/nodeに統一 ([b248415](https://github.com/akiojin/ollama-router/commit/b2484151f18ce6cf78cd3cd1d9e12ce3fbc52f4e)), closes [#96](https://github.com/akiojin/ollama-router/issues/96)


### Bug Fixes

* add support module to integration_gpu test harness ([d70a080](https://github.com/akiojin/ollama-router/commit/d70a08014c448a127f0a2f5c6e8b740f2781c81b))
* **agent:** advance ready_models on pull and resend heartbeat when all ready ([0c5f91b](https://github.com/akiojin/ollama-router/commit/0c5f91bd3e0fd763fba4be64d79664e5883535f1))
* **agent:** heartbeat uses placeholder metrics when collection fails ([87bf821](https://github.com/akiojin/ollama-router/commit/87bf8210c05481000a8821a706037d5dcc53ce51))
* **agent:** mark ready when counts reached and push initial heartbeat ([5076c71](https://github.com/akiojin/ollama-router/commit/5076c71a84408c90ab7e27c5c0ac2e033bd93749))
* **agent:** start api server before registration for health check ([f4e3241](https://github.com/akiojin/ollama-router/commit/f4e3241cb92cc0234f17aa61b77e991769d778a5))
* **agent:** stop forcing initializing true when some models not ready ([95973d5](https://github.com/akiojin/ollama-router/commit/95973d531eaa7a2b9989a189ba853aec6974163d))
* **auth:** openAI互換エンドポイントへのAPIキー認証適用とテスト修正 ([d9a09d4](https://github.com/akiojin/ollama-router/commit/d9a09d4d99920ec04cd1918761ead6e8020e25ff))
* **auth:** 認証機能のテスト修正とレスポンス改善 ([f0305fa](https://github.com/akiojin/ollama-router/commit/f0305fa50f4284c8371878b4a4dec5591b76b9cb))
* **build:** macOSでホストclang/arを利用 ([c7b2bed](https://github.com/akiojin/ollama-router/commit/c7b2bedbbdf1efa8d60590d064fc957e8b674b9b))
* **build:** macOSネイティブビルドをサポート ([b3e5f6f](https://github.com/akiojin/ollama-router/commit/b3e5f6f50ee11646670100297ab91907a9edc062))
* **clippy:** drop redundant AgentError conversion in heartbeat ([68850ce](https://github.com/akiojin/ollama-router/commit/68850cec1bd881e7f3c187f628377440fed529cf))
* **models:** use existing glm4 9b q4_K_M manifest and align UI/specs ([6932729](https://github.com/akiojin/ollama-router/commit/6932729c8d0dbe89abfed82586e872f45ba57b13))
* **proxy:** sync registry readiness when recording metrics ([e3fea5c](https://github.com/akiojin/ollama-router/commit/e3fea5ccf1f86a505354ecd954754123a8fb3e87))
* **proxy:** unify header types and init metrics fields ([c112d97](https://github.com/akiojin/ollama-router/commit/c112d97fc1fd00d1854dfb710ba8407dac38d27b))
* remove needless borrow in users_api_test ([155747e](https://github.com/akiojin/ollama-router/commit/155747ebecfef228c92fe697478b17543017dd76))
* **rename:** agent registryをnode registryに修正 ([c304ebc](https://github.com/akiojin/ollama-router/commit/c304ebc363900f5b7b1f6eaa0677f08a5bb8b336))
* **rename:** guiコードのアイコンファイル参照を修正 ([a64bd2e](https://github.com/akiojin/ollama-router/commit/a64bd2eba638b4e2724bebf39a704dbe751a8261))
* **rename:** rename作業の残存箇所を修正してテストを全て通過させる ([bcb4cf7](https://github.com/akiojin/ollama-router/commit/bcb4cf72f832674f046039b614049d4f37550f24))
* **rename:** support::coordinatorをsupport::routerに修正 ([9657d87](https://github.com/akiojin/ollama-router/commit/9657d87b493658194edd64a7dfa2290896b672c6))
* **rename:** テストコード内の環境変数名を統一 ([704a328](https://github.com/akiojin/ollama-router/commit/704a328bdf8acecf99729632612bb237d8f0f38f))
* **rename:** テストファイル内のクレート名参照を修正 ([39d9b25](https://github.com/akiojin/ollama-router/commit/39d9b257a983765486fe2efc633ab7166ce5431e))
* **rename:** リネーム漏れ修正によりCI失敗を解消 ([bd0104b](https://github.com/akiojin/ollama-router/commit/bd0104b30352cfadc8aa220169b9dea96e4a79c3))
* resolve clippy and dead_code warnings ([ea90f2a](https://github.com/akiojin/ollama-router/commit/ea90f2ab4724a2b8b848ef5a9e54b19e454bcf63))
* resolve duplicate module and clippy issues ([50286a5](https://github.com/akiojin/ollama-router/commit/50286a5425071e4ee5ed4561dc193326c77b3da4))
* resolve duplicate module errors in integration tests ([59ce8d6](https://github.com/akiojin/ollama-router/commit/59ce8d69fe44434994723a842906d0570ea58728))
* **test:** add db_pool and jwt_secret to all AppState initializations ([e1624eb](https://github.com/akiojin/ollama-router/commit/e1624eb914a37408c05caaf041551a9262208a77))
* **test:** configure tests to run with single thread ([dd8769b](https://github.com/akiojin/ollama-router/commit/dd8769b2defe8f2b0cbb5b053f156181047df426))
* **test:** contract testsでヘルスチェックをスキップし全テスト成功 ([d161809](https://github.com/akiojin/ollama-router/commit/d161809737126702ce22124a8f30ae259d864720))
* **test:** dashboard_gpu_displayテストの期待ステータスコードを修正 ([8a0c66a](https://github.com/akiojin/ollama-router/commit/8a0c66a57efdc343585d597d327c9ff492b6fa11))
* **test:** model_info_testの期待ステータスコードを修正 ([fe131a0](https://github.com/akiojin/ollama-router/commit/fe131a0c4836ba6223726aaf8841dbced363d000))
* **test:** openai互換APIテストのモックエンドポイントをv1パスに修正 ([4aeaa9b](https://github.com/akiojin/ollama-router/commit/4aeaa9b7f3789ff9a568ce24e1b63415f391499d))
* **test:** openAI互換エンドポイントのテストにAPIキー認証を追加 ([21df080](https://github.com/akiojin/ollama-router/commit/21df0804c459bff45f74b3afe59630cc279f84c3))
* **test:** openai互換性テストでtest-modelをロード済みモデルに追加 ([e200313](https://github.com/akiojin/ollama-router/commit/e200313c37bb870eeeb1b9bde817175ea3f74644))
* **test:** proxy_completions_queue_overflow_returns_503テストを一時的に無効化 ([de4b9d3](https://github.com/akiojin/ollama-router/commit/de4b9d3d7fef24f3c9868783a05b5800d0190b3e))
* **test:** resolve test isolation issues with AUTH_DISABLED env var ([285748d](https://github.com/akiojin/ollama-router/commit/285748d28fa72414ffd79c6b7b782228055c780b))
* **tests:** reference support module via crate to avoid duplicate loads ([2b17509](https://github.com/akiojin/ollama-router/commit/2b175098b9e2a806d2a642d2f632efbb6128956d))
* **tests:** silence missing-docs and unexpected cfg warnings ([bd29a5f](https://github.com/akiojin/ollama-router/commit/bd29a5f0c750e00906a77ba213cf259056813814))
* **ui,api:** address review feedback (logs route, csv, readiness render, banner/colspan) ([3e8a63c](https://github.com/akiojin/ollama-router/commit/3e8a63cb6ab100b7acd6f8a0985a1a29588e3656))
* **ui:** keep detail modals hidden until opened ([a6b0029](https://github.com/akiojin/ollama-router/commit/a6b0029c6a6b63f7c30306bc8593c1a52416d1bd))
* **ui:** keep modals hidden on load and cover with tests ([fec1040](https://github.com/akiojin/ollama-router/commit/fec104032bb111ee652f5be7eaa5a92febe36c0d))
* **ui:** open agent modal without reference errors ([51fef50](https://github.com/akiojin/ollama-router/commit/51fef50806b3fe2e7430403cff16316f070fa9cc))
* **ui:** show request timestamps in browser local time with TZ label ([19bc1c5](https://github.com/akiojin/ollama-router/commit/19bc1c518b51dafaf5ce4a303d1d5022f72bd9de))
* use fully qualified path for DownloadTaskManager ([a7ab672](https://github.com/akiojin/ollama-router/commit/a7ab67211f34b1631ee85d31541396963d831d17))


### Features

* **agent-api:** proxy openai traffic via agent endpoint ([fbdc3fa](https://github.com/akiojin/ollama-router/commit/fbdc3fa2cb264bf88e9fd4a5ebab431099d24ce4))
* **agent-init:** propagate initializing/ready_models in heartbeat ([f0bf8e9](https://github.com/akiojin/ollama-router/commit/f0bf8e9a7802afc430b599e0a431a90b1a080be9))
* **agent-pool:** add per-model ollama pool and route via agent api ([a453b53](https://github.com/akiojin/ollama-router/commit/a453b53496dfdd9e527035f2bf347b04167d91a0))
* **agent-reg:** parse agent /v1/models on register and sync readiness ([d6465aa](https://github.com/akiojin/ollama-router/commit/d6465aa4cefa7f69c97c5c69ba9bfc3993afbd11))
* **agent:** allow skipping models via OLLAMA_SKIP_MODELS ([60aebbc](https://github.com/akiojin/ollama-router/commit/60aebbcd6583d900330ec10733b1181f581e76a8))
* **agent:** bootstrap all coordinator models via pool ([8c6a239](https://github.com/akiojin/ollama-router/commit/8c6a239cd0c0301d39a1ae4d615e8b0a2cc35ed2))
* **agent:** t088-t090エージェント認証トークン統合を実装 ([daee161](https://github.com/akiojin/ollama-router/commit/daee161e37edd4b669794638841c754d9dbedf80))
* **auth:** add authentication endpoints and agent token support ([8fcd37d](https://github.com/akiojin/ollama-router/commit/8fcd37d0816b190ddeb6044c6dd44c8ecbcdd4bd))
* **auth:** implement password hashing and JWT (T032-T046 GREEN) ([72e9b03](https://github.com/akiojin/ollama-router/commit/72e9b03bd4243c0bbdf876ebbe4e843e706aa3a0))
* **auth:** t047-t056認証ミドルウェアとdb操作実装 ([5717eb4](https://github.com/akiojin/ollama-router/commit/5717eb4d2fe51af647de3280596caca76bc30958))
* **auth:** t057-t059認証api実装とappstate拡張 ([a53dd34](https://github.com/akiojin/ollama-router/commit/a53dd34b5a988c1bfbcf52119827f0b0dd3ab9c9))
* **auth:** t060-t063ユーザー管理api実装 ([6958fe7](https://github.com/akiojin/ollama-router/commit/6958fe7b17716509c61473e76678a0df2cc48789))
* **auth:** t064-t066 apiキー管理api実装 ([17de595](https://github.com/akiojin/ollama-router/commit/17de5952ed082173a46e7d1ba3dcbc829745dd09))
* **auth:** t067エージェント登録apiにagent_token追加 ([2a65c97](https://github.com/akiojin/ollama-router/commit/2a65c97496d0822ef230943ba385c0a399976e47))
* **auth:** t068-t070初回起動時の管理者作成処理を実装 ([60fc0e1](https://github.com/akiojin/ollama-router/commit/60fc0e1a5c910bfddef34ed67044c93906c6b71f))
* **auth:** t071-t074ルーター統合と認証ミドルウェア適用 ([a360860](https://github.com/akiojin/ollama-router/commit/a360860c9584b9b70444e89a3980381253e0d338))
* **auth:** マージ feature/authenticate into feature/rename ([7053963](https://github.com/akiojin/ollama-router/commit/705396351f09ba087b049576c1d6977dc41892fb))
* **auth:** 認証機能のセットアップ ([8b6048f](https://github.com/akiojin/ollama-router/commit/8b6048fbd9ba56299f70d5056fc1430d8e5b3dd0))
* **coord-error:** add service unavailable handling ([c7ab69d](https://github.com/akiojin/ollama-router/commit/c7ab69ddd604c35ecafbbb8e00cb7dc5f4d15c2e))
* **dashboard:** track uptime since last online ([3a557fd](https://github.com/akiojin/ollama-router/commit/3a557fde66caa59d65b93224de7541952f530384))
* **db:** implement SQLite migrations (T039-T041 GREEN) ([0e4ee72](https://github.com/akiojin/ollama-router/commit/0e4ee728a68eb3925986f5d058fb3331a83bd05d))
* **frontend:** t075-t087フロントエンド実装完了 ([693dba7](https://github.com/akiojin/ollama-router/commit/693dba725eb2dee06559a9ea9a6e5717e4bf150b))
* **logs:** add agent /api/logs and coordinator proxy integration ([4c93462](https://github.com/akiojin/ollama-router/commit/4c9346221314531d49ceee4bbc80cf5f9a8694de))
* **models:** align available list to required five models ([9feb85d](https://github.com/akiojin/ollama-router/commit/9feb85d029a7f28c46e2f1afa70c3669cda1c097))
* **models:** lock supported list to requested models ([c4a847c](https://github.com/akiojin/ollama-router/commit/c4a847c792b47628337188a29d59b2e63a84d941))
* **proxy:** block until ready agent (queue-like wait, max 1024 waiters) ([eee03dc](https://github.com/akiojin/ollama-router/commit/eee03dc11c8bacabf21b92d774071a77599497d1))
* **proxy:** route openai traffic via agent api and stream passthrough ([9c333ea](https://github.com/akiojin/ollama-router/commit/9c333eaf1390af0e68e487b3b46936b260265109))


### BREAKING CHANGES

* プロジェクト名、実行ファイル名、ストレージパス、環境変数名が変更されました。

既存ユーザーは以下の対応が必要です:

1. データ移行:
   * 旧: ~/.ollama-coordinator/
   * 新: ~/.or/

2. 環境変数更新:
   * `COORDINATOR_*`→`ROUTER_*`
   * `AGENT_*`→`NODE_*`

3. 実行ファイル名:
   * `ollama-coordinator-coordinator`→`or-router`
   * `ollama-coordinator-agent`→`or-node`

4. Docker/設定ファイル:
   * docker-compose.ymlのサービス名変更
   * 環境変数の更新

# [1.5.0](https://github.com/akiojin/ollama-router/compare/v1.4.1...v1.5.0) (2025-11-15)


### Bug Fixes

* **agent:** 設定フォームで空の数値入力を無視 ([c16a7fb](https://github.com/akiojin/ollama-router/commit/c16a7fb7e61e528c13fd9ff3278409ccb50207d7))
* **ui:** モデル管理タブの初期化を修正 ([3cac7b6](https://github.com/akiojin/ollama-router/commit/3cac7b69edc1002375e61f63d326f155d6a0e229))


### Features

* **agent:** enforce gpu usage ([49fe342](https://github.com/akiojin/ollama-router/commit/49fe342ae4a06330384b34099bd5c4ec24979b61))
* **api:** add openai compatibility endpoints ([705cbe9](https://github.com/akiojin/ollama-router/commit/705cbe9dd27d2dbfe79e24b7d59936c62faa6ca8))
* **balancer:** prioritize gpu-capable agents ([0163d79](https://github.com/akiojin/ollama-router/commit/0163d7999369bf254086ed9005cd1fa7ddd2066d))
* **coordinator:** log client ip in request history ([d00e56b](https://github.com/akiojin/ollama-router/commit/d00e56b1ac237b2c4967d891870e707285e8debc))
* **installer:** windowsアイコンを統一しメニュー登録 ([aa828bd](https://github.com/akiojin/ollama-router/commit/aa828bdf48fb200b46fe50dd6c4c3a570af82f0f))
* **logging:** ダッシュボードでノードログを確認できるようにする ([09cad04](https://github.com/akiojin/ollama-router/commit/09cad047b286286d402214439cfddf58f425074a))

## [1.4.1](https://github.com/akiojin/ollama-router/compare/v1.4.0...v1.4.1) (2025-11-14)


### Bug Fixes

* **coordinator:** ダッシュボードをバイナリに同梱 ([b8c6c7d](https://github.com/akiojin/ollama-router/commit/b8c6c7de14aec59a740f29d51d13511eff8dcbb4))

# [1.4.0](https://github.com/akiojin/ollama-router/compare/v1.3.1...v1.4.0) (2025-11-14)


### Bug Fixes

* **agent:** windows環境でのnvidia gpu検出を修正 ([ef9a8a8](https://github.com/akiojin/ollama-router/commit/ef9a8a8091ad8c44e7b014e1b7147d23bfce968e))
* **test:** windowsでnvidia gpu検出テストをスキップ ([9cd8a77](https://github.com/akiojin/ollama-router/commit/9cd8a7774d47eb6418424496dd28f4898c36e75e))


### Features

* **coordinator:** add system tray support ([78a0ad2](https://github.com/akiojin/ollama-router/commit/78a0ad24b02447dbfb6501ebeabccc758fb92c40))
* **coordinator:** add system tray support ([15ae54a](https://github.com/akiojin/ollama-router/commit/15ae54ade0f45310dab1c57e4e10b0e8c971f623))
* **tray:** refresh agent/coordinator icons ([0cf6f7d](https://github.com/akiojin/ollama-router/commit/0cf6f7df2cad819907f50087745615e38334afe6))
* **tray:** システムトレイアイコンを刷新 ([a44569f](https://github.com/akiojin/ollama-router/commit/a44569f48581b56ee065c805471f3ed6fe6da98d))

## [1.3.1](https://github.com/akiojin/ollama-router/compare/v1.3.0...v1.3.1) (2025-11-14)


### Bug Fixes

* **installer:** embed cab into windows msi ([9ad1797](https://github.com/akiojin/ollama-router/commit/9ad1797cea81222f0676d1147f4f95b94ee2ba1b))

# [1.3.0](https://github.com/akiojin/ollama-router/compare/v1.2.3...v1.3.0) (2025-11-14)


### Bug Fixes

* **installer:** mark windows components as 64-bit ([2c0e10f](https://github.com/akiojin/ollama-router/commit/2c0e10fac0dcd96ec2da8b7b619983d8059fb909))
* **lint:** needless_borrows_for_generic_argsエラーを修正 ([f05bc48](https://github.com/akiojin/ollama-router/commit/f05bc481166c05bc2f0c7c6a31359f5cc781ccb3))


### Features

* **agent:** ノード側HTTPサーバーとモデルプルAPI実装 (T033) ([2142f53](https://github.com/akiojin/ollama-router/commit/2142f53bc67d11550d7569bcc5b3c4e8848a36fb))
* **coordinator:** ノード登録時の自動モデル配布機能を実装 (T032) ([7935be0](https://github.com/akiojin/ollama-router/commit/7935be05098c0ad458609dc53af66f94caf442bb))
* **coordinator:** ルーター主導のモデル配布機能を実装 (Phase 3.1-3.3) ([62edaf7](https://github.com/akiojin/ollama-router/commit/62edaf7435e82cacd177ae3eb6939a0be72ff129))
* **error:** エラーハンドリング強化 (T039) ([5350a31](https://github.com/akiojin/ollama-router/commit/5350a31bd9dae6b6c7690d25767ab18e1c3a42cb))
* **logging:** ロギング強化 (T040) ([8cd8575](https://github.com/akiojin/ollama-router/commit/8cd85758bcaa5eeeef5e77e8d4dbb6ed8091f80e))
* **models:** 進捗報告機能とノード自動配布を実装 (T034, T032拡張) ([9c81c48](https://github.com/akiojin/ollama-router/commit/9c81c4894c25b6403feafc802faf6adabd0b7404))
* **ui:** モデル管理ダッシュボードUIを実装 (T036-T038) ([4b4ccb7](https://github.com/akiojin/ollama-router/commit/4b4ccb77261482550d9b9dcbebec4a196ec0ed0b))

## [1.2.3](https://github.com/akiojin/ollama-router/compare/v1.2.2...v1.2.3) (2025-11-14)


### Bug Fixes

* **ci:** allow wix downgrade on windows ([698c7f6](https://github.com/akiojin/ollama-router/commit/698c7f657517c21ba88fc8a0e145b93cbdd12aac))
* **ci:** configure macos x86 linker ([fe3af80](https://github.com/akiojin/ollama-router/commit/fe3af8042fd83a8a1ce2d19bc96928ec832a9ede))

## [1.2.2](https://github.com/akiojin/ollama-router/compare/v1.2.1...v1.2.2) (2025-11-14)


### Bug Fixes

* **ci:** macOSリンカ設定とbackmerge制御を復元 ([f5af5ae](https://github.com/akiojin/ollama-router/commit/f5af5ae002a395dcce30f4edaeea7dba2ba9ef8e))
* **docs:** stabilize markdownlint table rules ([7a86925](https://github.com/akiojin/ollama-router/commit/7a8692570885cb4b44e0027dafe2285032b90c24))
* **lint:** normalize markdown tables and lint tooling ([0af3f37](https://github.com/akiojin/ollama-router/commit/0af3f37e7caf1d4922e9e4f1ae66f986ee1e8167))

## [1.2.1](https://github.com/akiojin/ollama-router/compare/v1.2.0...v1.2.1) (2025-11-13)


### Bug Fixes

* **ci:** backmerge even if publish fails ([b0f54d8](https://github.com/akiojin/ollama-router/commit/b0f54d8d72de93acecc95d18e2811c4ca58b8c47))
* **ci:** set macos linker for publish ([f590be2](https://github.com/akiojin/ollama-router/commit/f590be2971dd2839758dd8ebe307a0cec749ef55))

# [1.2.0](https://github.com/akiojin/ollama-router/compare/v1.1.1...v1.2.0) (2025-11-12)


### Bug Fixes

* **agent:** address tray clippy warnings ([ee119e0](https://github.com/akiojin/ollama-router/commit/ee119e04c467339c5a940d982a45a52f7e3c62a1))
* **agent:** silence windows lint warnings ([17dcebc](https://github.com/akiojin/ollama-router/commit/17dcebc6f5a2f21ecbdfd3699fe28990a1fd95f5))
* satisfy clippy on tray module ([c5a38be](https://github.com/akiojin/ollama-router/commit/c5a38beaac113d2edfbb9b0a1e5a2c39f251f114))


### Features

* **agent:** add system tray gui and installers ([727fd2d](https://github.com/akiojin/ollama-router/commit/727fd2dc5f69ed577d474e0dfdd5b3eaf8b95894))
* **agent:** switch settings to env vars and panel ([a3fa0a3](https://github.com/akiojin/ollama-router/commit/a3fa0a30601b00d0ee541d2ff9682d9ea8d21d6c))
* **docker:** macOS SDKを使用したクロスコンパイル環境を追加 ([4b5b527](https://github.com/akiojin/ollama-router/commit/4b5b527c6456b6a5b2c1a8ca06987e696887997d))

## [1.1.1](https://github.com/akiojin/ollama-router/compare/v1.1.0...v1.1.1) (2025-11-11)


### Bug Fixes

* **docs:** improve README subtitle clarity ([151672a](https://github.com/akiojin/ollama-router/commit/151672a3b1b89638c1cc82ab6810f2609d88da0b))
* **lint:** disable MD001, MD012, MD025 for CHANGELOG compatibility ([c709a60](https://github.com/akiojin/ollama-router/commit/c709a60db66a2d0f2f472f9b913bd3f937f19acc))

# [1.1.0](https://github.com/akiojin/ollama-router/compare/v1.0.0...v1.1.0) (2025-11-11)


### Bug Fixes

* **ci:** correct required status checks with actual check names ([be4cef2](https://github.com/akiojin/ollama-router/commit/be4cef2d0d1dc229eb1f402892ced5d21d50e89d))
* **ci:** hookテストワークフローをpnpmに対応 ([0bd14e4](https://github.com/akiojin/ollama-router/commit/0bd14e489153785c25ae217b8b8593f7b2c02ffb))
* **ci:** pnpm/action-setup@v4のバージョン競合を解消 ([d151529](https://github.com/akiojin/ollama-router/commit/d1515291ba5a8172d82fce67cef21f4062f829e4))
* **ci:** restrict semantic-release to main branch only ([135f9fb](https://github.com/akiojin/ollama-router/commit/135f9fb9f966376903b7db097b0839357a059f81))
* **ci:** unify pnpm version to 10.20.0 across all workflows ([1808297](https://github.com/akiojin/ollama-router/commit/180829759629b7865c05dce5b2ec2345b5fcbf43))
* **ci:** unity-mcp-serverの設定でAuto Mergeワークフローを更新 ([2e27a43](https://github.com/akiojin/ollama-router/commit/2e27a433bdd4eb75ba7b4608755bf61584d3d2af))
* **docker:** change global package installation from pnpm to npm ([0d4ee13](https://github.com/akiojin/ollama-router/commit/0d4ee1349e73527252e4fcb46fd072264a58501c))
* **docker:** change global package installation from pnpm to npm ([b86a34f](https://github.com/akiojin/ollama-router/commit/b86a34f7fe2622fd83e083412cec7ddb690771e7))
* **docker:** update .codex volume mapping and sync auth.json from host ([340f409](https://github.com/akiojin/ollama-router/commit/340f409080e8944e6bd0f11b3237bc16dcd6f226))
* **docs:** CHANGELOGのmarkdownlintエラーを修正 ([b5036da](https://github.com/akiojin/ollama-router/commit/b5036da026a6aaa690368d2472933d06b0d1ba5d))
* **docs:** CHANGELOGのmarkdownlintエラーを再修正 ([38980ac](https://github.com/akiojin/ollama-router/commit/38980ac6c26f8642d30f1d067497d057a00aadd2))
* **docs:** CHANGELOGの連続空白行を修正し、worktreesを除外 ([9b4b7b0](https://github.com/akiojin/ollama-router/commit/9b4b7b00d98910157b78fbe01756e0f83a870c8b))
* **docs:** ドキュメントの表現を改善 ([44dba3e](https://github.com/akiojin/ollama-router/commit/44dba3eacce6bc1a98514f14da03f8d915adeb97))
* **release:** add GH_TOKEN to trigger release.yml workflow ([8bbbd89](https://github.com/akiojin/ollama-router/commit/8bbbd890d4bf84070b584b679b5af1caa0bb557b))
* **release:** add patch release test marker ([52ab19d](https://github.com/akiojin/ollama-router/commit/52ab19d5d9a3def30e5855808c180cf2bb7b4bdb))
* **release:** align workflows with unity repo ([7ec2363](https://github.com/akiojin/ollama-router/commit/7ec2363290560ce7a916f1b8caeabe4867e78fc7))
* **release:** append patch release verification note ([d1dec52](https://github.com/akiojin/ollama-router/commit/d1dec529c9047b1c8c84e2bbbb3da5941d61035a))
* **release:** explicitly trigger release.yml via gh workflow run ([bd990a4](https://github.com/akiojin/ollama-router/commit/bd990a4c8f0e26ebaf3efb86cfafdde0118f35ba))
* **release:** explicitly use PERSONAL_ACCESS_TOKEN for git push ([6b981c2](https://github.com/akiojin/ollama-router/commit/6b981c2108dc2cc28c58f7b8d7651668a49351e6))
* **release:** remove ANSI color codes before version extraction ([12afb18](https://github.com/akiojin/ollama-router/commit/12afb18c8b24d4ca4ba878d544c7840fc71b5c96))
* **release:** remove redundant release shell ([971c5ea](https://github.com/akiojin/ollama-router/commit/971c5ea1c6d8f65d6a6eeaadbd6566150121962c))
* **release:** revert to simple form matching unity-mcp-server v2.35.1 ([919a2d8](https://github.com/akiojin/ollama-router/commit/919a2d86648d486bee2b7deec26a27e2fb6f6a19))
* **release:** unity mcp serverフローに合わせて更新 ([df0c7e9](https://github.com/akiojin/ollama-router/commit/df0c7e9569e011268e48b112f240462dcf42d574))
* **release:** unity-mcp-serverと完全に同じリリースフローに統一 ([f4219d0](https://github.com/akiojin/ollama-router/commit/f4219d053512cf5149855dce332c855bbeadc52b))
* **release:** unity-mcp-serverと完全に同じリリースフローに統一 ([eb9da11](https://github.com/akiojin/ollama-router/commit/eb9da11ab728e9dfc65f0d93bfb26c3b62fdbab6))
* **release:** unity-mcp-serverと完全に同じリリースフローに統一 ([cc142b2](https://github.com/akiojin/ollama-router/commit/cc142b297487a143af7c1c802f2ddfa6b627adbf))
* **release:** wait and use REST API for workflow dispatch ([2d05fe9](https://github.com/akiojin/ollama-router/commit/2d05fe9a8c8de2af879b291305a9fc769ce9e779))
* **release:** yaml syntax error in merge commit message ([c32ccf7](https://github.com/akiojin/ollama-router/commit/c32ccf77d045297a22cb57eb8818097131166bfd))
* **workflow:** draftチェック検証を安定化 ([0975ec5](https://github.com/akiojin/ollama-router/commit/0975ec592f6c7cdafd0aa793074cb943e5d383a4))
* **workflow:** PAT必須でauto-merge権限を保証 ([f5aed47](https://github.com/akiojin/ollama-router/commit/f5aed47040ca097f46c05b37c3ab218fe69f084a))
* **workflow:** PERSONAL_ACCESS_TOKENを使用 ([0cb6e2c](https://github.com/akiojin/ollama-router/commit/0cb6e2c3d1ea59a183ae8838da2a34a3d5d66ee7))


### Features

* **docs:** add gpu-aware routing feature description ([c98e193](https://github.com/akiojin/ollama-router/commit/c98e1939f8bd5906da673c198167c15acd7d0793))
* **hooks:** claude-worktreeのフック機構を統合 ([27275e0](https://github.com/akiojin/ollama-router/commit/27275e05a89602f49c8e322a07a75892b1b6fa54))
* **release:** add automated release workflows ([23391e3](https://github.com/akiojin/ollama-router/commit/23391e38c64916288904e3273191f86068dd3b7e))
* **workflow:** auto-mergeフローをupstream同期 ([45c81be](https://github.com/akiojin/ollama-router/commit/45c81befa5c4eaa6523e5b1bee7a312d6120848d))

## [1.0.1-alpha.3](https://github.com/akiojin/ollama-router/compare/v1.0.1-alpha.2...v1.0.1-alpha.3) (2025-11-07)

### Bug Fixes

* **docker:** change global package installation from pnpm to npm ([b86a34f](https://github.com/akiojin/ollama-router/commit/b86a34f7fe2622fd83e083412cec7ddb690771e7))

## [1.0.1-alpha.2](https://github.com/akiojin/ollama-router/compare/v1.0.1-alpha.1...v1.0.1-alpha.2) (2025-11-07)

### Bug Fixes

* **docker:** update .codex volume mapping and sync auth.json from host ([340f409](https://github.com/akiojin/ollama-router/commit/340f409080e8944e6bd0f11b3237bc16dcd6f226))

## [1.0.1-alpha.1](https://github.com/akiojin/ollama-router/compare/v1.0.0...v1.0.1-alpha.1) (2025-11-06)

### Bug Fixes

* **docs:** CHANGELOGのmarkdownlintエラーを修正 ([b5036da](https://github.com/akiojin/ollama-router/commit/b5036da026a6aaa690368d2472933d06b0d1ba5d))
* **docs:** CHANGELOGのmarkdownlintエラーを再修正 ([38980ac](https://github.com/akiojin/ollama-router/commit/38980ac6c26f8642d30f1d067497d057a00aadd2))

# 1.0.0 (2025-11-06)

## Bug Fixes

* .gitattributesで改行をLFに統一 ([a7d3add](https://github.com/akiojin/ollama-router/commit/a7d3add456699819409e7775f5b3b9639c5c0c22))
* **agent:** default to cpu mode and detect premature ollama exit ([306e794](https://github.com/akiojin/ollama-router/commit/306e7941d3a9aa46be6b99e5f461e50b4ee3052b))
* **agent:** detect runtime arch for ollama downloads ([2b38b41](https://github.com/akiojin/ollama-router/commit/2b38b410d616ab013c6f3a4b62d20bf6acd9f117))
* **agent:** Docker for MacでApple Silicon GPUを検出可能に ([e80c2ac](https://github.com/akiojin/ollama-router/commit/e80c2acaf83f465938eb35e1fd2f338da4d680b7))
* **agent:** follow github ollama download and allow override ([71d5257](https://github.com/akiojin/ollama-router/commit/71d525755fd2aa84c6f81e9bfb45b71150f1da9a))
* **agent:** ollama psでollamaバイナリの正しいパスを使用 ([5c04857](https://github.com/akiojin/ollama-router/commit/5c0485758e62eedf8865ec306067d215c00b3f2d))
* **agent:** pick ollama archive based on architecture ([bc8578a](https://github.com/akiojin/ollama-router/commit/bc8578a9336f2109fd49ee060367737abe3deceb))
* **agent:** point to archive assets for downloads ([db41214](https://github.com/akiojin/ollama-router/commit/db412148aade1f78225f3b76ed0e572178bcc0d2))
* **agent:** set user-agent for ollama download ([5ec8dc8](https://github.com/akiojin/ollama-router/commit/5ec8dc8b17dfc8c59efb8df47108cfe76aa8cb7e))
* **api:** clippyが警告するテストモジュール配置を調整 ([16720df](https://github.com/akiojin/ollama-router/commit/16720df8516b16d24c8218c21101f2232c182a34))
* **api:** openai proxy returns upstream errors ([549ae48](https://github.com/akiojin/ollama-router/commit/549ae484565017d299f9fa7842c60128f9d68b5a))
* **api:** ダッシュボード静的配信のルーターとテストを修正 ([02b7a24](https://github.com/akiojin/ollama-router/commit/02b7a246a47ad3d0ee62c263f5aece70cc68dfec))
* AppErrorにDebugトレイトを追加 ([1321b3d](https://github.com/akiojin/ollama-router/commit/1321b3db996094c15d7ac960c58ee2bf59aaed6b))
* await_holding_lock警告を修正（tokio::sync::Mutexを使用） ([8b9d6ba](https://github.com/akiojin/ollama-router/commit/8b9d6ba5720e22a60fc5e0d84ce9e2b17d225342))
* booleanアサーションのclippy警告を修正 ([a682508](https://github.com/akiojin/ollama-router/commit/a68250866e9acd2d778353653b1d5f968d27abc2))
* **checks:** check-tasks.shを引数オプショナルに変更 ([8ec63a6](https://github.com/akiojin/ollama-router/commit/8ec63a6631a38bc8c8daaa8d17c3e4e5835b9892))
* **checks:** check-tasks.shを引数オプショナルに変更 ([0706868](https://github.com/akiojin/ollama-router/commit/0706868c86d7e021adfcadf6e8b3e61258e64aa8))
* **ci:** commitlint設定を簡素化 - @commitlint/config-conventional依存を削除 ([0b56135](https://github.com/akiojin/ollama-router/commit/0b561354c6aab4e4e1f6c667a9868f3309914571))
* **ci:** disable pipefail for tar|head pipe ([89b699a](https://github.com/akiojin/ollama-router/commit/89b699ae150c65046838af937e1b6e24fb6eea65))
* **ci:** tar pipe fix ([2f0cdd6](https://github.com/akiojin/ollama-router/commit/2f0cdd6c0e6fe1ff9c7ec9b3462a4550b50d2757))
* **ci:** tar+headパイプブレイクエラーを修正 ([f2227a3](https://github.com/akiojin/ollama-router/commit/f2227a3d7770b6385f0676e7188412d76d560f2c))
* **ci:** tar|headパイプでpipefailを一時無効化 ([d8b1264](https://github.com/akiojin/ollama-router/commit/d8b1264bda2f7359169a87251bad6bff2b27411b))
* **ci:** ブランチ操作ブロックの堅牢化 ([699ac51](https://github.com/akiojin/ollama-router/commit/699ac518012f3eeb1f4403df6b13126c134fd1fd))
* clippy指摘を解消 ([f0cba57](https://github.com/akiojin/ollama-router/commit/f0cba578679e5f6d6dac899d979666e865989f3e))
* clippy警告とドキュメントコメント混在を修正 ([f9fca44](https://github.com/akiojin/ollama-router/commit/f9fca44a4fb616a7a7035dc271b8d5107b4b8734))
* clippy警告の解消とメトリクス更新の抽象化 ([6052d0d](https://github.com/akiojin/ollama-router/commit/6052d0d1e4eb1d1ed4532f9bc0358e50fe1284c5))
* commitlint ジョブを常に実行 ([188dca3](https://github.com/akiojin/ollama-router/commit/188dca3a2cde4190155b35c4bc897e07ad14b8c7))
* **coordinator:** allow multiple agents per machine/port ([ae9cf73](https://github.com/akiojin/ollama-router/commit/ae9cf73e321b25495c6567f0d69aa478af6121c7))
* **dashboard:** Chart.jsのテキスト色をより明るく調整 ([cfcdf23](https://github.com/akiojin/ollama-router/commit/cfcdf23f9b84bd318535a53ff39be9b729bde8d3)), closes [#e2e8f0](https://github.com/akiojin/ollama-router/issues/e2e8f0)
* **dashboard:** GPUモデル名を表示してユーザー体験を改善 ([6d9ee31](https://github.com/akiojin/ollama-router/commit/6d9ee3129395f326dc5b69b283de41cd04f1d03b))
* **dashboard:** GPU情報をDashboardAgentに追加 ([9216198](https://github.com/akiojin/ollama-router/commit/921619811ee3abc7f466f05b7e2998e1d6f14de4))
* **dashboard:** renderAgents中の未定義変数エラーを解消 ([ee48afe](https://github.com/akiojin/ollama-router/commit/ee48afed8906eb4a5d04141537d714e40e3fa37a))
* **dashboard:** ダークモードでのテキスト可視性を改善 ([97f8808](https://github.com/akiojin/ollama-router/commit/97f880848be26689b7479a1cf207824191c65967))
* **dashboard:** 静的アセットの404とChart.js SRIを修正 ([9a76c68](https://github.com/akiojin/ollama-router/commit/9a76c68014648db194e7349c411fd478916fdca8))
* **lint:** Clippyエラーとテスト失敗を修正 ([fb13bbf](https://github.com/akiojin/ollama-router/commit/fb13bbf18d240682919eab0b49aa6c8d41c093b6))
* markdownlintエラーを修正 ([3b9ce0d](https://github.com/akiojin/ollama-router/commit/3b9ce0d4f7476cf41530bd63b21623a8d21501a1))
* **quality:** 品質チェック対応＆テスト修正（T033-T037） ([ed919f2](https://github.com/akiojin/ollama-router/commit/ed919f241664f835b2fc0576693673ecbc4c1ae5))
* **release:** rebuild artifacts after version bump ([c5e59be](https://github.com/akiojin/ollama-router/commit/c5e59bee76ab46235073f987d1ac582c77c5bfe4))
* **release:** release-binaries.ymlの環境変数評価を修正 ([b126adc](https://github.com/akiojin/ollama-router/commit/b126adcd76d1d447c06bcbd532dd2e3d0f4c6f5e))
* **release:** release-binaries.ymlの環境変数評価を修正 ([a1bd31e](https://github.com/akiojin/ollama-router/commit/a1bd31e7de1fee200d2067b9ab4d246b5e224de7))
* **release:** tarアーカイブ検証でのstdout書き込みエラーを修正 ([3a7e5c9](https://github.com/akiojin/ollama-router/commit/3a7e5c97d1b5bbbf6369da063b25b0f1697ec4f7))
* **release:** tarアーカイブ検証のstdout書き込みエラー修正 ([5f4d37f](https://github.com/akiojin/ollama-router/commit/5f4d37f4e3dcbc13da7bc21441fafc7ef18af0e1))
* **scripts:** finish-feature.shのブランチ名制限を緩和 ([0b70969](https://github.com/akiojin/ollama-router/commit/0b70969267fa6d2f7107e07edbee4aee19ed0d40))
* **specify:** Worktree内でのSPEC作成をサポート ([d1f9244](https://github.com/akiojin/ollama-router/commit/d1f9244a2d2776eff61bcb2e66ef9d5c30c35637))
* SQLiteをJSONファイルストレージに置き換え ([d03285f](https://github.com/akiojin/ollama-router/commit/d03285fd0c46fcfdc2c095af8a91454cd8b2df77))
* **storage:** 破損したagents.jsonを自動復旧 ([e3249b7](https://github.com/akiojin/ollama-router/commit/e3249b7f649def1431917bdf53ddf00d152dfde8))
* **test:** clippy警告を修正 - manual_range_containsルールに対応 ([e030a55](https://github.com/akiojin/ollama-router/commit/e030a55bafe06befc9088ae0b5ab8d25d934377c))
* **test:** contract testsの未使用変数警告を修正 ([d01220f](https://github.com/akiojin/ollama-router/commit/d01220fff92637386e909b187f502d8282fd12bc))
* **test:** GPU必須検証エラーメッセージのテスト期待値を修正 ([32dab36](https://github.com/akiojin/ollama-router/commit/32dab36192876af772a2fb0dc1e4c4bebdcc98b8))
* **ui:** リクエスト履歴セクションの見出しとレイアウトを改善 ([72772cb](https://github.com/akiojin/ollama-router/commit/72772cbe67669767eddfc566b1e5e799764e6374))
* **ui:** リクエスト履歴を新しい順に表示 ([fde64d9](https://github.com/akiojin/ollama-router/commit/fde64d94d29f8e839b2c117875e9ad6e1da59046))
* コードフォーマットを修正 ([4a756aa](https://github.com/akiojin/ollama-router/commit/4a756aaa23920027c50657e0ac4955ca6f9c5537))
* コンパイルエラーとフォーマット問題を修正 ([e257b7a](https://github.com/akiojin/ollama-router/commit/e257b7a52cfe3672163e05aae774563238c14d3a))
* ブランチ自動作成を無効化（ユーザーが手動でブランチ作成する） ([e7c31f6](https://github.com/akiojin/ollama-router/commit/e7c31f61a2c5dc9cdd6b4555fa643876b0613e1e))
* 残りのclippy警告をすべて修正 ([9c6ee9d](https://github.com/akiojin/ollama-router/commit/9c6ee9d2e712bbbca18915210f2e93c35a33cf7e))
* 自動マージでチェック完了を待機 ([da98a51](https://github.com/akiojin/ollama-router/commit/da98a51dce278b14a96cf71544503aacd9024b73))

## Features

* **agent:** Agent構造体にGPU能力フィールドを追加してダッシュボード表示対応 ([7e293ab](https://github.com/akiojin/ollama-router/commit/7e293ab37467b55327ce7e0dbbc5eec6e5c3c382))
* **agent:** allow configurable startup timeout and cpu fallback ([a847211](https://github.com/akiojin/ollama-router/commit/a847211461fde2bc9b7eae1053793dcd16974cca))
* **agent:** extract ollama archives on download ([ca9af7c](https://github.com/akiojin/ollama-router/commit/ca9af7c34db58519edf66c1e565be3c8d2f60861))
* **agent:** GPU検出PoCを作成しollama ps検出を削除 ([9170e68](https://github.com/akiojin/ollama-router/commit/9170e68144e696dce9e21c0ab572deaea0d90712))
* **agent:** GPU検出ロジックを強化（PoCからの統合） ([185544f](https://github.com/akiojin/ollama-router/commit/185544f2fab6a3f56f72f299ef81b95a827d8f27))
* **agent:** improve registration retry and dashboard layout ([973096b](https://github.com/akiojin/ollama-router/commit/973096b50667210598adf97561206771d111a434))
* **agent:** ollama psコマンドによるGPU検出を追加 ([46f1541](https://github.com/akiojin/ollama-router/commit/46f154102f784cc08a38735b6c9dd8683e98448b))
* **agent:** Ollama自動ダウンロードのチェックサム検証機能実装 ([215b5b5](https://github.com/akiojin/ollama-router/commit/215b5b50e6a59bc444a3a6c66b91103cf9525f99))
* **agent:** Ollama自動ダウンロードのプロキシ対応実装 ([f3b2c18](https://github.com/akiojin/ollama-router/commit/f3b2c189b2a51a1018840d67e2f8b4843431c30f))
* **agent:** Ollama自動ダウンロードのリトライ機能実装 ([1f93d74](https://github.com/akiojin/ollama-router/commit/1f93d74cc181a2f98b72eac082c9039b501eecf5))
* **agent:** Ollama自動ダウンロードの進捗表示機能実装 ([e3fb1ce](https://github.com/akiojin/ollama-router/commit/e3fb1ceb259be6bd8649b05dd6727e9fd873b81e))
* **agent:** pull_model()にリトライ機能を統合 ([4784665](https://github.com/akiojin/ollama-router/commit/478466515824fc638edd1186049f1f996deeee74))
* **agent:** pull_model()に進捗表示機能を統合 ([cda8961](https://github.com/akiojin/ollama-router/commit/cda89615f01755aedb80553ee19d86c5e2fd83a6))
* **agent:** T058 - Ollama自動管理実装（自動ダウンロード・起動） ([04f886d](https://github.com/akiojin/ollama-router/commit/04f886d07db1d4a796372f984b1b95e3ce611355))
* **agent:** T059-T062 - Agent基本機能実装（登録・ハートビート・メトリクス） ([2e3b9d0](https://github.com/akiojin/ollama-router/commit/2e3b9d0fac6adbdcf822d525e5b2d2918544379c))
* **agent:** ノード削除フローを実装 ([ef9d28f](https://github.com/akiojin/ollama-router/commit/ef9d28f2cef3a11cad76febb924a38012858c516))
* **agent:** カスタム設定の永続化を実装 ([2589219](https://github.com/akiojin/ollama-router/commit/25892190feb9adbddd76918b67acf133493c3f28))
* **agent:** 強制切断APIとUI対応 ([d8b3072](https://github.com/akiojin/ollama-router/commit/d8b307278068cec54e817f72c837f52a7ead1917))
* **api:** expose per-agent metrics history ([c28b9d7](https://github.com/akiojin/ollama-router/commit/c28b9d771e60cbac0ed28cd39cae56ae5944478c))
* **api:** ノード設定APIを追加 ([6a760d4](https://github.com/akiojin/ollama-router/commit/6a760d4f2f78752a9acf7b4fb8e77d456db004c4))
* **balancer:** implement load-aware balancing ([5c4da94](https://github.com/akiojin/ollama-router/commit/5c4da94e10e8751e12083848608050f85b234b2d))
* **balancer:** improve summary averaging ([db2fca3](https://github.com/akiojin/ollama-router/commit/db2fca30d8aa9dc22fed37eaaff779e1caffac08))
* **balancer:** prefer lower latency agents ([0f61057](https://github.com/akiojin/ollama-router/commit/0f61057adc2834cf0a433dbb9702032a54e21ef4))
* commitlint設定とci.ymlバックアップ作成 ([2056223](https://github.com/akiojin/ollama-router/commit/205622335d1349b38dbdad545332be9a1c679fa6))
* commitlint設定とci.ymlバックアップ作成 ([de2c34e](https://github.com/akiojin/ollama-router/commit/de2c34e53937c5522e3d0cfaba56604f46fc08b0))
* **common:** Phase 3.3完了 - Common層実装 (T019-T026) ([3c64c09](https://github.com/akiojin/ollama-router/commit/3c64c09271e6b59a12da11c2793b61279dbed446))
* **coordinator:** AppStateにRequestHistoryStorage追加 ([83d1d5e](https://github.com/akiojin/ollama-router/commit/83d1d5ebddf4b82d8f29a1eac56b74836c47e4ed))
* **coordinator:** GPU検証エラーとログの改善 ([746c617](https://github.com/akiojin/ollama-router/commit/746c6173ec4d7df097b698949ae6b687f685a30c))
* **coordinator:** return json validation errors ([bd78d3a](https://github.com/akiojin/ollama-router/commit/bd78d3ac071f5483c55d3c0d3a4c6513bae0bd4d))
* **coordinator:** Setup タスク完了（T001-T003） ([145074b](https://github.com/akiojin/ollama-router/commit/145074b6cc98ffea07b241c722c7f4f0ce4658b4))
* **coordinator:** T027-T031 - Agent登録API実装（Contract Test T009対応） ([e87ce99](https://github.com/akiojin/ollama-router/commit/e87ce99e7f8a7ba415e33e50ae652ef77f25978d))
* **coordinator:** T032, T034 - ヘルスチェックAPI実装（Contract Test T010対応） ([f43242a](https://github.com/akiojin/ollama-router/commit/f43242a3c6a590c38814f6397f87b70089775d10))
* **coordinator:** T036-T039 - ProxyAPI実装（Contract Test T011-T012対応） ([28a2ca3](https://github.com/akiojin/ollama-router/commit/28a2ca3691fb88436ff014a9e9c6979700cb74fb))
* **coordinator:** T040-T041 - Agent一覧API実装（Contract Test T013対応） ([eef5a9f](https://github.com/akiojin/ollama-router/commit/eef5a9f9430cdbd20a79ac34b02e57bd49f451d0))
* **coordinator:** T042-T045 - DB永続化実装完了 ([303ca71](https://github.com/akiojin/ollama-router/commit/303ca71d39a140bc26201410d53f8e6d8debf41c))
* **coordinator:** T046-T048 - バックグラウンドヘルスモニター実装 ([5d970f6](https://github.com/akiojin/ollama-router/commit/5d970f6598d7e8a3505b4b9b4f7bee2a7854e010))
* **coordinator:** ストレージ層実装完了（T014-T019） ([68a60bc](https://github.com/akiojin/ollama-router/commit/68a60bc37760bde1503c449db035211881bf8408))
* **coordinator:** メトリクスAPIハンドラー実装 (T016) - Phase 2.3完了 ([438242c](https://github.com/akiojin/ollama-router/commit/438242cf5512b124a7fd73b56512b509dab868a8))
* **coordinator:** メトリクスベースロードバランシングのコア実装 (T011-T015) ([753c1c7](https://github.com/akiojin/ollama-router/commit/753c1c7bb38636ba0612475ef8db485c89da4eb8))
* **coordinator:** メトリクスベースロードバランシングの基盤実装 ([cbee350](https://github.com/akiojin/ollama-router/commit/cbee35095d630b70d487d2bb8c577d083951ad6d))
* **coordinator:** リクエスト/レスポンス履歴保存機能を実装 ([6655cba](https://github.com/akiojin/ollama-router/commit/6655cba3439ff801c728b86da9f28e19f727f566))
* **coordinator:** 環境変数でロードバランサーモード切り替え実装 (T017-T019) - Phase 2.4完了 ([fe6e428](https://github.com/akiojin/ollama-router/commit/fe6e428c6dedfa153a7c9f93c21357181cc7c1aa))
* **dashboard:** GPU能力スコアをダッシュボードに表示 ([cc2d991](https://github.com/akiojin/ollama-router/commit/cc2d991639c2e84d724ca4d91aa90d5abe506b16))
* **dashboard:** GPU非対応時に「非対応」と表示 ([1df423f](https://github.com/akiojin/ollama-router/commit/1df423f826fe1bb324fae807daadd758ab5a51c3))
* **dashboard:** graph agent cpu and memory metrics ([3ad6866](https://github.com/akiojin/ollama-router/commit/3ad686696e1dba911a9a80ab15174d9c8d02a216))
* **dashboard:** surface gpu averages in stats ([3b6f8ff](https://github.com/akiojin/ollama-router/commit/3b6f8ff77bf07a12998c49fd02016a2a8f8ede94))
* **dashboard:** surface gpu metrics in ui ([5a714f2](https://github.com/akiojin/ollama-router/commit/5a714f21c0973d152b51677f7c448179dd448f68))
* **dashboard:** ノード一括操作に向けた選択UIを追加 ([b5a65b7](https://github.com/akiojin/ollama-router/commit/b5a65b72acd21723d34dacf766ff92fad08e9c9c))
* **dashboard:** ノード一覧のソートを実装 ([abc6a0e](https://github.com/akiojin/ollama-router/commit/abc6a0e56ff579d67bde12f9c91b453332ac74d1))
* **dashboard:** ノード一覧のフィルタを追加 ([eb8778e](https://github.com/akiojin/ollama-router/commit/eb8778e15790d6c31f2dd90e39fb7ec1d02d647d))
* **dashboard:** ノード設定UIの骨組みを追加 ([7af0a35](https://github.com/akiojin/ollama-router/commit/7af0a356e0a5284578a7d992933e21cd543135ff))
* **dashboard:** ノード詳細モーダルを追加 ([baaa309](https://github.com/akiojin/ollama-router/commit/baaa309f6bf570717e1ab3a204f1583d825a7184))
* **dashboard:** エクスポート機能と強制切断UIを追加 ([afd3933](https://github.com/akiojin/ollama-router/commit/afd3933c95c30a740a1ce4251d0a8cf0828b3e6a))
* **dashboard:** ダッシュボードAPIとUIを追加 ([607bc2f](https://github.com/akiojin/ollama-router/commit/607bc2fb251e521383544dff880d03b07e178fb9))
* **dashboard:** ページネーションを追加 ([74d815e](https://github.com/akiojin/ollama-router/commit/74d815e90299da1c1f9fa0846f8bc204cea6b10d))
* **dashboard:** リクエスト履歴API実装（T023-T026） ([59e5fc0](https://github.com/akiojin/ollama-router/commit/59e5fc0c5ee857db0ffb0ae5fbec1709f2b2d654))
* **dashboard:** リクエスト履歴チャートを追加 ([b4a593e](https://github.com/akiojin/ollama-router/commit/b4a593e76a1073a1fee49c5b09729a7366edd1fa))
* **dashboard:** 取得と描画時間を可視化 ([e97bfed](https://github.com/akiojin/ollama-router/commit/e97bfedbab8319a21202603ec289f4c45938698d))
* **dashboard:** 概要APIでポーリングを1リクエストに集約 ([72626ac](https://github.com/akiojin/ollama-router/commit/72626ac549cff2b5ca663232a7fdf24a8b6d91fe))
* **gpu:** Apple Silicon GPU検出をMetal APIで実装 ([2363d35](https://github.com/akiojin/ollama-router/commit/2363d3581d266838e2c0a2f00abf3bce8c1dd774))
* **gpu:** Docker環境用に環境変数でGPU情報を設定可能に ([d38cb23](https://github.com/akiojin/ollama-router/commit/d38cb234a3c172827e1bc6b204cc18e1823c7c2e))
* **gpu:** GPU必須ノード登録要件の実装 ([c9c7e1f](https://github.com/akiojin/ollama-router/commit/c9c7e1f816662ee353ccfe092bfc217bab6113a7))
* **gpu:** GPU能力スコア機能を追加 ([cd3147a](https://github.com/akiojin/ollama-router/commit/cd3147ab5a70b225fde43bc98eb0180a1de21d6b))
* **gpu:** GPU詳細メトリクス3フィールド追加 ([5fcc69e](https://github.com/akiojin/ollama-router/commit/5fcc69e237b0910db5d475a9dd00c7e521074a2e))
* GPU登録要件を全体に適用 ([42e9afd](https://github.com/akiojin/ollama-router/commit/42e9afde69ec38dcf80b544f6b5c5e08d05c8e8f))
* **heartbeat:** track response time metrics ([91d90a3](https://github.com/akiojin/ollama-router/commit/91d90a394aa7bab5b8d721478becbdd9db41a6c1))
* improve gpu fallback and docker memory ([3ddfdc7](https://github.com/akiojin/ollama-router/commit/3ddfdc76503448f3c6f28d5a83c816f0fc2136c4))
* **infra:** developブランチ作成とブランチ保護設定完了 ([ad3a83a](https://github.com/akiojin/ollama-router/commit/ad3a83a560957bd75771594224308af1d3ff02b2))
* **metrics:** add gpu telemetry pipeline ([16c4e68](https://github.com/akiojin/ollama-router/commit/16c4e68c1a6dbe96789d63e98615c00b1ffc5866))
* **metrics:** annotate snapshots with freshness ([b96d7c6](https://github.com/akiojin/ollama-router/commit/b96d7c64c321d0822b66005cb203d6b8fd172128))
* **metrics:** expose metrics summary endpoint ([166ddbd](https://github.com/akiojin/ollama-router/commit/166ddbd67f1b22ed848c716ee5f16d1657eb1a76))
* **metrics:** refine metric freshness handling ([10c717e](https://github.com/akiojin/ollama-router/commit/10c717eec8ddd619cb8ae6db5b39626da7fd09e8))
* **plan:** Ollama Router Systemの実装計画を作成 ([ea2da6b](https://github.com/akiojin/ollama-router/commit/ea2da6b2689d58f8879240b74d59646765e501b4))
* **proxy:** stream openai responses ([27178c6](https://github.com/akiojin/ollama-router/commit/27178c6b58ee3dd1b4126bfffc99e77c8fe1111e))
* **proxy:** リクエスト/レスポンスキャプチャ実装（T020-T022） ([120fc91](https://github.com/akiojin/ollama-router/commit/120fc911c49c503056e43f635c4ee2504f7e7c1b))
* **release:** 完全自動化リリースシステムの実装 ([70a9f2e](https://github.com/akiojin/ollama-router/commit/70a9f2edaf47ddf79028952394c6ab0120aa3376))
* **setup:** Phase 3.1完了 - Cargo Workspaceセットアップ (T001-T008) ([e3f5b64](https://github.com/akiojin/ollama-router/commit/e3f5b64a43d8f31753421af4bbf45240ff229662))
