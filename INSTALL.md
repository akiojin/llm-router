# INSTALL

## Prerequisites
- Linux/macOS/Windows x64 (GPU推奨、GPUなしは登録不可)
- Rust toolchain (nightly不要) と cargo
- Docker (任意、コンテナ利用時)
- CUDAドライバ (GPU使用時。NVIDIAのみ)

## 1) Rustソースからビルド（推奨）
```bash
git clone https://github.com/akiojin/llm-router.git
cd llm-router
make quality-checks   # fmt/clippy/test/markdownlint 一式
cargo build -p llm-router --release
```
生成物: `target/release/llm-router`

## 2) Docker で起動
```bash
docker build -t llm-router:latest .
docker run --rm -p 8080:8080 --gpus all \
  -e OPENAI_API_KEY=... \
  llm-router:latest
```
GPUを使わない場合は `--gpus all` を外すか、`CUDA_VISIBLE_DEVICES=""` を設定。

## 3) C++ Node ビルド

```bash
npm run build:node

# 手動でビルドする場合:
cd node
cmake -B build -S .
cmake --build build --config Release
```

生成物: `node/build/llm-node`

## 4) 基本設定

### ルーター（Rust）環境変数

| 環境変数 | デフォルト | 説明 |
|---------|-----------|------|
| `LLM_ROUTER_HOST` | `0.0.0.0` | バインドアドレス |
| `LLM_ROUTER_PORT` | `8080` | リッスンポート |
| `LLM_ROUTER_DATABASE_URL` | `sqlite:~/.llm-router/router.db` | データベースURL |
| `LLM_ROUTER_JWT_SECRET` | 自動生成 | JWT署名シークレット |
| `LLM_ROUTER_ADMIN_USERNAME` | `admin` | 初期管理者ユーザー名 |
| `LLM_ROUTER_ADMIN_PASSWORD` | - | 初期管理者パスワード |
| `LLM_ROUTER_LOG_LEVEL` | `info` | ログレベル |
| `LLM_ROUTER_HEALTH_CHECK_INTERVAL` | `30` | ヘルスチェック間隔（秒） |
| `LLM_ROUTER_NODE_TIMEOUT` | `60` | ノードタイムアウト（秒） |
| `LLM_ROUTER_LOAD_BALANCER_MODE` | `auto` | ロードバランサーモード |

クラウドAPI:

- `OPENAI_API_KEY`, `GOOGLE_API_KEY`, `ANTHROPIC_API_KEY`

### ノード（C++）環境変数

| 環境変数 | デフォルト | 説明 |
|---------|-----------|------|
| `LLM_ROUTER_URL` | `http://127.0.0.1:11434` | ルーターURL |
| `LLM_NODE_PORT` | `11435` | HTTPサーバーポート |
| `LLM_NODE_MODELS_DIR` | `~/.ollama/models` | モデルディレクトリ |
| `LLM_NODE_BIND_ADDRESS` | `0.0.0.0` | バインドアドレス |
| `LLM_NODE_HEARTBEAT_SECS` | `10` | ハートビート間隔（秒） |
| `LLM_NODE_ALLOW_NO_GPU` | `false` | GPU必須を無効化 |
| `LLM_NODE_LOG_LEVEL` | `info` | ログレベル |
| `LLM_NODE_LOG_DIR` | `~/.llm-router/logs` | ログディレクトリ |

**注意**: 旧環境変数名（`ROUTER_HOST`, `LLM_MODELS_DIR`等）は非推奨です。
新しい環境変数名を使用してください。

## 5) 起動例
```bash
# ルーター
cargo run -p llm-router

# ノード (別シェル)
./node/build/llm-node
```

## 6) 動作確認
- ダッシュボード: `http://localhost:8080/dashboard`
- 健康チェック: `curl http://localhost:8080/api/health`
- OpenAI互換: `curl http://localhost:8080/v1/models`

## 7) 品質チェック（必須）
```bash
make quality-checks
```
上記がすべて成功してからコミット・プッシュすること。
