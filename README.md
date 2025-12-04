# LLM Router

A centralized management system for coordinating LLM inference nodes across multiple machines

English | [日本語](./README.ja.md)

## Overview

LLM Router is a powerful centralized system that provides unified management and a single API endpoint for multiple LLM inference nodes running across different machines. It features intelligent load balancing, automatic failure detection, real-time monitoring capabilities, and seamless integration for enhanced scalability.

## Key Features

- **Unified API Endpoint**: Access multiple LLM runtime instances through a single URL
- **Automatic Load Balancing**: Intelligently distribute requests across available agents
- **Automatic Failure Detection**: Detect offline agents and exclude them from distribution
- **Real-time Monitoring**: Comprehensive visualization of agent states and performance metrics via web dashboard
- **Request History Tracking**: Complete request/response logging with 7-day retention
- **Self-registering Agents**: Agents automatically register with the Coordinator
- **Model Auto-Distribution**: Automatically distribute AI models to agents based
  on GPU memory capacity
- **WebUI Management**: Manage agent settings, monitoring, and control through
  browser-based dashboard
- **Cross-Platform Support**: Works on Windows 10+, macOS 12+, and Linux
- **GPU-Aware Routing**: Intelligent request routing based on GPU capabilities
  and availability
- **Cloud Model Prefixes**: Add `openai:` `google:` or `anthropic:` in the
  model name to proxy to the corresponding cloud provider while keeping the
  same OpenAI-compatible endpoint.

Quick references: [INSTALL](./INSTALL.md) / [USAGE](./USAGE.md) /
[TROUBLESHOOTING](./TROUBLESHOOTING.md)

## Quick Start

### Router (llm-router)

```bash
# Build
cargo build --release -p llm-router

# Run
./target/release/llm-router
# Default: http://0.0.0.0:8080

# Access dashboard
# Open http://localhost:8080/dashboard in browser
```

**Environment Variables:**

| Variable | Default | Description |
|----------|---------|-------------|
| `LLM_ROUTER_HOST` | `0.0.0.0` | Bind address |
| `LLM_ROUTER_PORT` | `8080` | Listen port |
| `LLM_ROUTER_LOG_LEVEL` | `info` | Log level |
| `LLM_ROUTER_JWT_SECRET` | (auto-generated) | JWT signing secret |
| `LLM_ROUTER_ADMIN_USERNAME` | `admin` | Initial admin username |
| `LLM_ROUTER_ADMIN_PASSWORD` | (required) | Initial admin password |

**Backward compatibility:** Legacy env var names (`ROUTER_PORT` etc.) are supported but deprecated.

**System Tray (Windows/macOS only):**

On Windows 10+ and macOS 12+, the router displays a system tray icon.
Double-click to open the dashboard. Docker/Linux runs as a headless CLI process.

### Node (C++)

**Prerequisites:**

```bash
# macOS
brew install cmake

# Ubuntu/Debian
sudo apt install cmake build-essential

# Windows
# Download from https://cmake.org/download/
```

**Build & Run:**

```bash
# Build (Metal is enabled by default on macOS)
npm run build:node

# Run
npm run start:node

# Or manually:
# cd node && cmake -B build -S . && cmake --build build --config Release
# LLM_ROUTER_URL=http://localhost:8080 ./node/build/llm-node
```

**Environment Variables:**

| Variable | Default | Description |
|----------|---------|-------------|
| `LLM_ROUTER_URL` | `http://127.0.0.1:8080` | Router URL to register with |
| `LLM_NODE_PORT` | `11435` | Node listen port |
| `LLM_NODE_MODELS_DIR` | `~/.runtime/models` | Model storage directory |
| `LLM_NODE_BIND_ADDRESS` | `0.0.0.0` | Bind address |
| `LLM_NODE_HEARTBEAT_SECS` | `10` | Heartbeat interval (seconds) |
| `LLM_NODE_ALLOW_NO_GPU` | `false` | Allow running without GPU |
| `LLM_NODE_LOG_LEVEL` | `info` | Log level |

**Backward compatibility:** Legacy env var names (`LLM_MODELS_DIR` etc.) are supported but deprecated.

**Docker:**

```bash
# Build
docker build --build-arg CUDA=cpu -t llm-node:latest node/

# Run
docker run --rm -p 11435:11435 \
  -e LLM_ROUTER_URL=http://host.docker.internal:8080 \
  llm-node:latest
```

## Load Balancing

LLM Router supports multiple load balancing strategies to optimize request distribution across agents.

### Strategies

#### 1. Metrics-Based Load Balancing (Recommended)

Selects agents based on real-time metrics (CPU usage, memory usage, active requests). This intelligent mode provides optimal performance by dynamically routing requests to the least loaded agent, ensuring efficient resource utilization.

**Configuration:**
```bash
# Enable metrics-based load balancing
LOAD_BALANCER_MODE=metrics cargo run -p llm-router
```

**Load Score Calculation:**
```
score = cpu_usage + memory_usage + (active_requests × 10)
```

The agent with the **lowest score** is selected. If all agents have CPU usage > 80%, the system automatically falls back to round-robin.

**Example:**
- Agent A: CPU 20%, Memory 30%, Active 1 → Score = 60 ✓ Selected
- Agent B: CPU 70%, Memory 50%, Active 5 → Score = 170

#### 2. Advanced Load Balancing (Default)

Combines multiple factors including response time, active requests, and CPU usage to provide sophisticated agent selection with adaptive performance optimization.

**Configuration:**
```bash
# Use default advanced load balancing (or omit LOAD_BALANCER_MODE)
LOAD_BALANCER_MODE=auto cargo run -p llm-router
```

### Metrics API

Agents can report their metrics to the Coordinator for load balancing decisions.

**Endpoint:** `POST /api/agents/:id/metrics`

**Request:**
```json
{
  "agent_id": "550e8400-e29b-41d4-a716-446655440000",
  "cpu_usage": 45.5,
  "memory_usage": 60.2,
  "active_requests": 3,
  "avg_response_time_ms": 250.5,
  "timestamp": "2025-11-02T10:00:00Z"
}
```

**Response:** `204 No Content`

## Architecture

### System Overview

```
┌─────────────────────────────────────────────────────────────┐
│                          Client                              │
│                (Users, Applications, etc.)                   │
└────────────────────┬────────────────────────────────────────┘
                     │ POST /api/chat
                     │ POST /api/generate
                     ▼
┌─────────────────────────────────────────────────────────────┐
│                      Coordinator                             │
│                  (Central Management Server)                 │
│                                                               │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ 1. Select Agent (Load Balancing)                    │   │
│  │ 2. Proxy Request to Selected Agent                   │   │
│  │ 3. Return Response to Client                         │   │
│  └─────────────────────────────────────────────────────┘   │
└────┬──────────────────┬──────────────────┬─────────────────┘
     │                  │                  │
     │ Internal Proxy   │ Internal Proxy   │ Internal Proxy
     ▼                  ▼                  ▼
┌─────────┐        ┌─────────┐        ┌─────────┐
│ Agent 1 │        │ Agent 2 │        │ Agent 3 │
│         │        │         │        │         │
│  LLM runtime │        │  LLM runtime │        │  LLM runtime │
│  (Auto) │        │  (Auto) │        │  (Auto) │
└─────────┘        └─────────┘        └─────────┘
Machine 1          Machine 2          Machine 3
```

### Communication Flow (Proxy Pattern)

LLM Router uses a **Proxy Pattern** - clients only need to know the Router URL.

#### Traditional Method (Without Coordinator)
```bash
# Direct access to each LLM runtime - manual distribution by users
curl http://machine1:11434/api/chat -d '...'
curl http://machine2:11434/api/chat -d '...'
curl http://machine3:11434/api/chat -d '...'
```

#### With Coordinator (Proxy)
```bash
# Unified access to Coordinator - automatic distribution to optimal LLM runtime
curl http://coordinator:8080/api/chat -d '...'
curl http://coordinator:8080/api/chat -d '...'
curl http://coordinator:8080/api/chat -d '...'
```

**Detailed Request Flow:**

1. **Client → Coordinator**
   ```
   POST http://coordinator:8080/api/chat
   Content-Type: application/json

   {"model": "llama2", "messages": [...]}
   ```

2. **Coordinator Internal Processing**
   - Select optimal Agent/LLM runtime (Load Balancing)
   - Forward request to selected Agent's LLM runtime via HTTP client

3. **Coordinator → Agent (Internal Communication)**
   ```
   POST http://agent1:11434/api/chat
   Content-Type: application/json

   {"model": "llama2", "messages": [...]}
   ```

4. **Agent → LLM runtime → Agent (Local Processing)**
   - Agent forwards request to local LLM runtime instance
   - LLM runtime processes LLM and generates response

5. **Coordinator → Client (Return Response)**
   ```json
   {
     "id": "chatcmpl-xxx",
     "object": "chat.completion",
     "choices": [{
       "index": 0,
       "message": {"role": "assistant", "content": "..."},
       "finish_reason": "stop"
     }]
   }
   ```

> **Note**: LLM Router exclusively supports **OpenAI-compatible API format**.
> All responses follow the OpenAI Chat Completions API specification.

**From Client's Perspective**:
- Coordinator appears as the only LLM runtime API server
- No need to be aware of multiple internal LLM runtime instances
- Complete with a single HTTP request

### Benefits of Proxy Pattern

1. **Unified Endpoint**
   - Clients only need to know the Coordinator URL
   - No need to know each Agent/LLM runtime location

2. **Transparent Load Balancing**
   - Coordinator automatically selects optimal agent
   - Clients benefit from load distribution without awareness

3. **Automatic Retry on Failure**
   - If Agent1 fails → Coordinator automatically tries Agent2
   - No re-request needed from client

4. **Security**
   - Agent IP addresses not exposed to clients
   - Only Coordinator needs to be publicly accessible

5. **Scalability**
   - Adding Agents automatically increases processing capacity
   - No changes needed on client side

## Project Structure

```
llm-router/
├── common/              # Common library (types, protocols, errors)
│   ├── src/
│   │   ├── types.rs     # Agent, HealthMetrics, Request types
│   │   ├── protocol.rs  # Communication protocol definitions
│   │   ├── config.rs    # Configuration structures
│   │   └── error.rs     # Unified error types
│   └── Cargo.toml
├── coordinator/         # Coordinator server
│   ├── src/
│   │   ├── api/         # REST API handlers
│   │   │   ├── agent.rs    # Agent registration & list
│   │   │   ├── health.rs   # Health check receiver
│   │   │   └── proxy.rs    # LLM runtime proxy
│   │   ├── registry/    # Agent state management
│   │   ├── db/          # Database access
│   │   └── main.rs
│   ├── migrations/      # Database migrations
│   └── Cargo.toml
├── node/                # C++ Node (llama.cpp integrated)
│   ├── src/
│   │   ├── main.cpp     # Entry point
│   │   ├── api/         # OpenAI-compatible API
│   │   ├── core/        # llama.cpp inference engine
│   │   └── models/      # Model management
│   ├── tests/           # TDD tests
│   └── CMakeLists.txt
└── specs/               # Specifications (Spec-Driven Development)
    └── SPEC-32e2b31a/
        ├── spec.md      # Feature specification
        ├── plan.md      # Implementation plan
        └── tasks.md     # Task breakdown
```

## Dashboard

The dashboard ships with the coordinator process. Once the server is running you can supervise all registered agents, review recent request history, and manage metadata from your browser.

### Quick usage

1. Start the coordinator (inside Docker or on the host):
   ```bash
   cargo run -p llm-router
   ```
2. Open the dashboard in your browser:
   ```
   http://localhost:8080/dashboard
   ```
3. Filter, search, sort, and page through the agent list. Click “詳細” to edit display name, tags, or notes, or to force-disconnect / delete an agent. Use the export buttons above the table to download the current view as JSON or CSV.

For a deeper walkthrough, including API references and customisation tips, see [docs/dashboard.md](./docs/dashboard.md).

## Installation

### Requirements

- **Coordinator**: Linux / Windows 10+ / macOS 12+, Rust 1.70+
- **Agent**: Windows 10+ / macOS 12+ (CLI-based application), Rust 1.70+
- **GPU**: NVIDIA / AMD / Apple Silicon GPU required for agent registration
  - Automatically detected on startup
  - Docker for Mac: Apple Silicon detection supported
- **Docker memory**: When running via `docker-compose`, allocate at least 16 GiB RAM to the container (`mem_limit: 16g`, `mem_reservation: 13g`, `memswap_limit: 18g`). On Docker Desktop, open **Settings → Resources** and raise the memory slider to ≥16 GiB before running `docker compose up`. Without this, large models such as `gpt-oss:20b` will fail to start with "requires more system memory" errors.
- **LLM runtime**: Automatically downloaded and installed when not present
  - Progress display during download
  - Automatic retry on network errors
  - SHA256 checksum verification
  - Proxy support (HTTP_PROXY, HTTPS_PROXY environment variables)
- **LLM Models**: Automatically downloaded on first startup
  - Memory-based model selection (appropriate model size for available RAM)
  - Real-time progress display with streaming status updates
  - Automatic retry on network errors
  - Models pulled via LLM runtime API
- **Management**: Browser-based WebUI dashboard for agent settings and monitoring

### Coordinator Setup

```bash
# Clone repository
git clone https://github.com/your-org/llm-router.git
cd llm-router

# Build Coordinator
cd coordinator
cargo build --release

# Start Coordinator
./target/release/llm-router
# Default: http://0.0.0.0:8080
```

### Agent Setup

```bash
# Build Agent
cd agent
cargo build --release

# Start Agent (環境変数で上書き)
ROUTER_URL=http://coordinator-host:8080 ./target/release/llm-node

# 環境変数を指定しない場合はローカル設定パネルで保存した値、なければ http://localhost:8080
./target/release/llm-node
```

**Note**: LLM runtime is automatically downloaded and installed on first startup if not already present. The agent will:
- Detect the platform (Linux, macOS, Windows)
- Download the appropriate LLM runtime binary
- Verify integrity with SHA256 checksum
- Install to `~/.runtime-agent/bin/`
- **Automatically download LLM models**:
  - Memory-based model selection (chooses appropriate model size)
  - Real-time progress display during model download
  - Automatic retry on network errors (configurable via environment variables)
  - Streaming response processing for live status updates
- Start LLM runtime and register with the coordinator

Manual installation is also supported. Download LLM runtime from [runtime.ai](https://runtime.ai).

#### System tray (Windows / macOS)

- On Windows 10+ and macOS 12+, both the **agent** *and* the **coordinator** expose tray / menu bar icons when launched as binaries.
- The agent tray icon behaves as before: double-click or **Open Settings** to launch the local settings panel, edit coordinator URL / LLM runtime port / heartbeat interval, and jump to `ROUTER_URL/dashboard`. **Quit Agent** stops the background process. Linux builds continue to run as a headless CLI daemon (settings URL is printed to stdout).
- The coordinator tray icon lets you open the local dashboard (`http://127.0.0.1:<port>/dashboard` by default) or exit the server directly from the system tray. Double-clicking the icon also launches the dashboard in your default browser.
- Tray icons are derived from [Open Iconic](https://github.com/iconic/open-iconic) (MIT License); a copy of the license is included at `assets/icons/ICON-LICENSE.txt`.

### Release Automation

We follow the same release-branch workflow as `akiojin/unity-mcp-server`, with integrated binary building and publishing in `publish.yml`.

1. While on `develop`, run the `/release` slash command or execute `./scripts/create-release-branch.sh`. The helper script calls `gh workflow run create-release.yml --ref develop`, which performs a semantic-release dry-run and creates `release/vX.Y.Z`.
2. Pushing `release/vX.Y.Z` triggers `.github/workflows/release.yml`. That workflow runs semantic-release for real, updates CHANGELOG/Cargo manifests, creates the Git tag and GitHub Release, merges the release branch into `main`, backmerges `main` into `develop`, and deletes the release branch.
3. The `main` push kicks off `.github/workflows/publish.yml`, which builds and attaches Linux/macOS/Windows archives to the GitHub Release.
   - During this phase the workflow also builds platform installers: macOS gets `or-router-<platform>.pkg` via `pkgbuild`, while Windows receives `or-router-<platform>.msi` via WiX. These ship alongside the existing `.tar.gz` / `.zip` archives so current release consumers stay unaffected.

Monitor the pipeline with:

```bash
gh run watch $(gh run list --workflow=create-release.yml --limit 1 --json databaseId --jq '.[0].databaseId')
gh run watch $(gh run list --workflow=release.yml --limit 1 --json databaseId --jq '.[0].databaseId')
gh run watch $(gh run list --workflow=publish.yml --limit 1 --json databaseId --jq '.[0].databaseId')
```

Only the `/release` invocation is manual; versioning, CHANGELOG generation, tagging, release creation, and binary distribution are fully automated.

#### GPU Detection

Agents automatically detect GPU on startup. **GPU is required** for agent registration.

**Supported GPUs:**
- **NVIDIA**: Detected via NVML library or device files (`/dev/nvidia0`)
- **AMD**: Detected via sysfs KFD Topology (`/sys/class/kfd/kfd/topology/nodes`)
- **Apple Silicon**: Detected via `lscpu`, `/proc/cpuinfo`, or Metal API (M1/M2/M3/M4)

**Docker for Mac Support:**
- Apple Silicon is automatically detected in Docker containers
- No additional configuration required

**Manual Configuration (Fallback):**

If automatic detection fails, set environment variables:

```bash
LLM_GPU_AVAILABLE=true \
LLM_GPU_MODEL="Your GPU Model" \
LLM_GPU_COUNT=1 \
./target/release/llm-node
```

## Usage

### Basic Usage

1. **Start Coordinator**
   ```bash
   cd coordinator
   cargo run --release
   ```

2. **Start Agents on Multiple Machines**
   ```bash
   # Machine 1
   ROUTER_URL=http://coordinator:8080 cargo run --release --bin llm-node

   # Machine 2
   ROUTER_URL=http://coordinator:8080 cargo run --release --bin llm-node

   # Machine 3
   ROUTER_URL=http://coordinator:8080 cargo run --release --bin llm-node
   ```

3. **Use LLM runtime API Through Coordinator**
   ```bash
   # Chat API
   curl http://coordinator:8080/api/chat \
     -H "Content-Type: application/json" \
     -d '{
       "model": "llama2",
       "messages": [{"role": "user", "content": "Hello!"}],
       "stream": false
     }'

   # Generate API
   curl http://coordinator:8080/api/generate \
     -H "Content-Type: application/json" \
     -d '{
       "model": "llama2",
       "prompt": "Tell me a joke",
       "stream": false
     }'
   ```

4. **List Registered Agents**
   ```bash
   curl http://coordinator:8080/api/agents
   ```

### Environment Variables

#### Router (llm-router)

| Variable | Default | Description |
|----------|---------|-------------|
| `LLM_ROUTER_HOST` | `0.0.0.0` | Bind address |
| `LLM_ROUTER_PORT` | `8080` | Listen port |
| `LLM_ROUTER_DATABASE_URL` | `sqlite://~/.llm-router/router.db` | Database URL |
| `LLM_ROUTER_LOG_LEVEL` | `info` | Log level |
| `LLM_ROUTER_HEALTH_CHECK_INTERVAL` | `30` | Health check interval (seconds) |
| `LLM_ROUTER_NODE_TIMEOUT` | `60` | Node timeout (seconds) |
| `LLM_ROUTER_LOAD_BALANCER_MODE` | `auto` | Load balancer mode (`metrics` or `auto`) |
| `LLM_ROUTER_JWT_SECRET` | (auto-generated) | JWT signing key (overridable via env var) |
| `LLM_ROUTER_ADMIN_USERNAME` | `admin` | Initial admin username |
| `LLM_ROUTER_ADMIN_PASSWORD` | (required) | Initial admin password (first run only) |
| `LLM_ROUTER_OPENAI_API_KEY` | - | OpenAI API key |
| `LLM_ROUTER_ANTHROPIC_API_KEY` | - | Anthropic API key |
| `LLM_ROUTER_GOOGLE_API_KEY` | - | Google API key |

**Backward Compatibility**: Legacy variable names (`ROUTER_PORT`, etc.) are still
supported but deprecated. A warning is logged when used.

#### Node (llm-node)

| Variable | Default | Description |
|----------|---------|-------------|
| `LLM_ROUTER_URL` | `http://127.0.0.1:8080` | Router URL to register with |
| `LLM_NODE_PORT` | `11435` | Node listen port |
| `LLM_NODE_IP` | (auto-detected) | Node IP address |
| `LLM_NODE_MODELS_DIR` | `~/.runtime/models` | Model storage directory |
| `LLM_NODE_LOG_LEVEL` | `info` | Log level |
| `LLM_NODE_LOG_DIR` | `~/.llm-node/logs` | Log directory |
| `LLM_NODE_LOG_RETENTION_DAYS` | `7` | Log retention days |
| `LLM_NODE_HEARTBEAT_SECS` | `10` | Heartbeat interval (seconds) |
| `LLM_NODE_ALLOW_NO_GPU` | `false` | Allow running without GPU |
| `LLM_NODE_BIND_ADDRESS` | `0.0.0.0` | Bind address |
| `LLM_NODE_MODEL_IDLE_TIMEOUT` | `300` | Model idle timeout (seconds) |
| `LLM_NODE_MAX_LOADED_MODELS` | `1` | Max loaded models |
| `LLM_NODE_MAX_MEMORY_BYTES` | (auto) | Max memory usage |
| `LLM_NODE_AUTO_REPAIR` | `true` | Auto repair |
| `LLM_NODE_REPAIR_TIMEOUT_SECS` | `60` | Repair timeout (seconds) |
| `LLM_NODE_CONFIG` | - | Config file path |

**Backward Compatibility**: Legacy variable names (`LLM_MODELS_DIR`, etc.) are
still supported but deprecated. A warning is logged when used.

## Development

### Commit Hooks

Install the JavaScript tooling once per clone to enable Husky-managed commit hooks:

```bash
pnpm install
```

- Runs the `prepare` script and configures Husky's Git hook directory.
- Adds a `commit-msg` hook that executes `commitlint --edit "$1"` so invalid messages fail locally instead of in CI.
- Use `pnpm run lint:commits` to lint a range manually (defaults to `origin/main..HEAD`).

### Running Tests

```bash
# Run all tests
cargo test --workspace

# Coordinator tests
cd coordinator
cargo test

# Agent tests
cd agent
cargo test

# Integration tests (including ignored, requires Coordinator server)
cd agent
TEST_ROUTER_URL=http://localhost:8080 cargo test --test integration_tests -- --ignored

# Full quality gate (fmt, clippy, workspace tests, specify checks, markdownlint, OpenAI proxy)
make quality-checks

# (Optional) Run only the OpenAI-compatible proxy regression suite
make openai-tests
```

### Spec-Driven Development

This project follows Spec-Driven Development:

1. `/speckit.specify` - Create feature specification
2. `/speckit.plan` - Create implementation plan
3. `/speckit.tasks` - Break down into tasks
4. Execute tasks (strict TDD cycle)

See [CLAUDE.md](./CLAUDE.md) for details.

### Claude Code Worktree Hooks

This project uses Claude Code PreToolUse Hooks to enforce Worktree environment
boundaries and prevent accidental operations that could disrupt the development workflow.

**Features:**

- **Git Branch Protection**: Blocks `git checkout`, `git switch`, `git worktree`
commands to prevent branch switching
- **Directory Navigation Control**: Blocks `cd` commands that would move outside
the Worktree boundary
- **Smart Allow Lists**: Permits read-only operations like `git branch --list`
- **Fast Execution**: Average response time < 50ms (target: < 100ms)

**Installation & Configuration:**

For detailed setup instructions, manual testing examples, and troubleshooting, see:

- [Quickstart Guide](./specs/SPEC-dc648675/quickstart.md) - Step-by-step setup
and verification
- [Feature Specification](./specs/SPEC-dc648675/spec.md) - Requirements and
acceptance criteria
- [Implementation Plan](./specs/SPEC-dc648675/plan.md) - Technical design and
architecture
- [Performance Report](./specs/SPEC-dc648675/performance.md) - Benchmark results

**Running Hook Tests:**

```bash
# Run all Hook contract tests (13 test cases)
make test-hooks

# Or run manually with Bats
npx bats tests/hooks/test-block-git-branch-ops.bats tests/hooks/test-block-cd-command.bats

# Run performance benchmark
tests/hooks/benchmark-hooks.sh
```

**Automated Testing:**

Hook tests are automatically executed in CI/CD:

- GitHub Actions: `.github/workflows/test-hooks.yml` (standalone)
- Quality Checks: `.github/workflows/quality-checks.yml` (integrated)
- Makefile: `make quality-checks` includes `test-hooks` target

## Request History

LLM Router automatically logs all requests and responses for debugging,
auditing, and analysis purposes.

### Features

- **Complete Request/Response Logging**: Captures full request bodies,
response bodies, and metadata
- **Automatic Retention**: Keeps history for 7 days with automatic cleanup
- **Web Dashboard**: View, filter, and search request history through the
web interface
- **Export Capabilities**: Export history in JSON or CSV format
- **Filtering Options**: Filter by model, agent, status, and time range

### Accessing Request History

#### Via Web Dashboard

1. Open the coordinator dashboard: `http://localhost:8080/dashboard`
2. Navigate to the "Request History" section
3. Use filters to narrow down specific requests
4. Click on any request to view full details including request/response bodies

#### Via API

**List Request History:**
```bash
GET /api/dashboard/request-responses?page=1&per_page=50
```

**Get Request Details:**
```bash
GET /api/dashboard/request-responses/{id}
```

**Export History:**
```bash
# JSON format
GET /api/dashboard/request-responses/export

# CSV format (via dashboard UI)
```

### Storage

Request history is stored in JSON format at:
- Linux/macOS: `~/.llm-router/request_history.json`
- Windows: `%USERPROFILE%\.llm-router\request_history.json`

The file is automatically managed with:
- Atomic writes (temp file + rename) to prevent corruption
- File locking to handle concurrent access
- Automatic cleanup of records older than 7 days

## API Specification

### Coordinator API

#### POST /api/agents
Register an agent.

**Request:**
```json
{
  "machine_name": "my-machine",
  "ip_address": "192.168.1.100",
  "runtime_version": "0.1.0",
  "runtime_port": 11434,
  "gpu_available": true,
  "gpu_devices": [
    { "model": "NVIDIA RTX 4090", "count": 2 }
  ]
}
```

**Response:**
```json
{
  "agent_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "registered"
}
```

#### GET /api/agents
Get list of registered agents.

**Response:**
```json
[
  {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "machine_name": "my-machine",
    "ip_address": "192.168.1.100",
    "runtime_version": "0.1.0",
    "runtime_port": 11434,
    "status": "online",
    "registered_at": "2025-10-30T12:00:00Z",
    "last_seen": "2025-10-30T12:05:00Z",
    "gpu_available": true,
    "gpu_devices": [
      { "model": "NVIDIA RTX 4090", "count": 2 }
    ],
    "gpu_count": 2,
    "gpu_model": "NVIDIA RTX 4090"
  }
]
```

#### POST /api/health
Receive health check information (Agent→Coordinator).

**Request:**
```json
{
  "agent_id": "550e8400-e29b-41d4-a716-446655440000",
  "cpu_usage": 45.5,
  "memory_usage": 60.2,
  "active_requests": 3
}
```

#### POST /api/agents/:id/metrics
Update agent metrics for load balancing (Agent→Coordinator).

**Request:**
```json
{
  "agent_id": "550e8400-e29b-41d4-a716-446655440000",
  "cpu_usage": 45.5,
  "memory_usage": 60.2,
  "active_requests": 3,
  "avg_response_time_ms": 250.5,
  "timestamp": "2025-11-02T10:00:00Z"
}
```

**Response:** `204 No Content`

#### GET /api/models/available

Get list of available models for distribution.

**Response:**

```json
{
  "models": [
    {
      "name": "gpt-oss:20b",
      "display_name": "GPT OSS (20B)",
      "size_gb": 12.5,
      "description": "Large language model, 20 billion parameters"
    }
  ],
  "source": "runtime_library"
}
```

#### POST /api/models/distribute

Distribute a model to one or more agents.

**Request:**

```json
{
  "model_name": "gpt-oss:7b",
  "target": "all"
}
```

Or for specific agents:

```json
{
  "model_name": "gpt-oss:7b",
  "target": "specific",
  "agent_ids": [
    "550e8400-e29b-41d4-a716-446655440000",
    "660f9511-f39c-52e5-c827-557766551111"
  ]
}
```

**Response:**

```json
{
  "task_ids": [
    "770ea622-g49d-63f6-d938-668877662222",
    "880fb733-h59e-74g7-e049-779988773333"
  ]
}
```

#### POST /api/agents/:id/models/pull

Instruct a specific agent to pull a model.

**Request:**

```json
{
  "model_name": "llama3.2:3b"
}
```

**Response:**

```json
{
  "task_id": "990gc844-i69f-85h8-f150-880099884444"
}
```

#### GET /api/agents/:id/models

Get list of installed models on a specific agent.

**Response:**

```json
[
  {
    "name": "gpt-oss:20b",
    "size_gb": 12.5,
    "installed_at": "2025-11-14T10:00:00Z"
  }
]
```

#### GET /api/tasks/:id

Get progress of a model download task.

**Response:**

```json
{
  "id": "770ea622-g49d-63f6-d938-668877662222",
  "agent_id": "550e8400-e29b-41d4-a716-446655440000",
  "model_name": "gpt-oss:7b",
  "status": "downloading",
  "progress": 0.45,
  "download_speed_bps": 10485760,
  "created_at": "2025-11-14T10:00:00Z",
  "updated_at": "2025-11-14T10:05:30Z"
}
```

#### POST /api/chat

Proxy endpoint for Chat API (OpenAI-compatible format).

**Request:**

```json
{
  "model": "gpt-oss:20b",
  "messages": [{"role": "user", "content": "Hello!"}],
  "stream": false
}
```

**Response (OpenAI-compatible):**

```json
{
  "id": "chatcmpl-xxx",
  "object": "chat.completion",
  "choices": [{
    "index": 0,
    "message": {"role": "assistant", "content": "Hello! How can I help you?"},
    "finish_reason": "stop"
  }]
}
```

> **Important**: LLM Router only supports OpenAI-compatible response format.
> Ollama-native format (`message`/`done` fields) is NOT supported.

#### POST /api/generate

Proxy endpoint for Generate API (OpenAI-compatible format).

**Request:**

```json
{
  "model": "gpt-oss:20b",
  "prompt": "Tell me a joke",
  "stream": false
}
```

**Response (OpenAI-compatible):**

```json
{
  "id": "cmpl-xxx",
  "object": "text_completion",
  "choices": [{
    "text": "Why did the programmer quit? Because he didn't get arrays!",
    "index": 0,
    "finish_reason": "stop"
  }]
}
```

## License

MIT License

## Contributing

Issues and Pull Requests are welcome.

For detailed development guidelines, see [CLAUDE.md](./CLAUDE.md).
### Cloud model prefixes (OpenAI-compatible API)

- Supported prefixes: `openai:`, `google:`, `anthropic:` (alias `ahtnorpic:`)
- Usage: set `model` to e.g. `openai:gpt-4o`, `google:gemini-1.5-pro`, `anthropic:claude-3-opus`
- Environment variables:
  - `OPENAI_API_KEY` (required), `OPENAI_BASE_URL` (optional, default `https://api.openai.com`)
  - `GOOGLE_API_KEY` (required), `GOOGLE_API_BASE_URL` (optional, default `https://generativelanguage.googleapis.com/v1beta`)
  - `ANTHROPIC_API_KEY` (required), `ANTHROPIC_API_BASE_URL` (optional, default `https://api.anthropic.com`)
- Behavior: prefix is stripped before forwarding; responses remain OpenAI-compatible. Streaming is passthrough as SSE.
- Metrics: `/metrics/cloud` exports Prometheus text with per-provider counters (`cloud_requests_total{provider,status}`) and latency histogram (`cloud_request_latency_seconds{provider}`).
