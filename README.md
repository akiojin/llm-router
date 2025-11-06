# Ollama Coordinator

A centralized management system for Ollama instances across multiple machines

English | [日本語](./README.ja.md)

## Overview

Ollama Coordinator is a system that provides unified management and a single API endpoint for multiple Ollama instances running across different machines. It features load balancing, automatic failure detection, and real-time monitoring.

## Key Features

- **Unified API Endpoint**: Access multiple Ollama instances through a single URL
- **Automatic Load Balancing**: Automatically distribute requests across available agents
- **Automatic Failure Detection**: Detect offline agents and exclude them from distribution
- **Real-time Monitoring**: Visualize all agent states via web dashboard
- **Request History Tracking**: Complete request/response logging with 7-day retention
- **Self-registering Agents**: Agents automatically register with the Coordinator
- **WebUI Management**: Manage agent settings, monitoring, and control through browser-based dashboard
- **Cross-Platform Support**: Works on Windows 10+, macOS 12+, and Linux

## Load Balancing

Ollama Coordinator supports multiple load balancing strategies to optimize request distribution across agents.

### Strategies

#### 1. Metrics-Based Load Balancing (Recommended)

Selects agents based on real-time metrics (CPU usage, memory usage, active requests). This mode provides optimal performance by routing requests to the least loaded agent.

**Configuration:**
```bash
# Enable metrics-based load balancing
LOAD_BALANCER_MODE=metrics cargo run -p ollama-coordinator-coordinator
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

Combines multiple factors including response time, active requests, and CPU usage for sophisticated agent selection.

**Configuration:**
```bash
# Use default advanced load balancing (or omit LOAD_BALANCER_MODE)
LOAD_BALANCER_MODE=auto cargo run -p ollama-coordinator-coordinator
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
│  Ollama │        │  Ollama │        │  Ollama │
│  (Auto) │        │  (Auto) │        │  (Auto) │
└─────────┘        └─────────┘        └─────────┘
Machine 1          Machine 2          Machine 3
```

### Communication Flow (Proxy Pattern)

Ollama Coordinator uses a **Proxy Pattern** - clients only need to know the Coordinator URL.

#### Traditional Method (Without Coordinator)
```bash
# Direct access to each Ollama - manual distribution by users
curl http://machine1:11434/api/chat -d '...'
curl http://machine2:11434/api/chat -d '...'
curl http://machine3:11434/api/chat -d '...'
```

#### With Coordinator (Proxy)
```bash
# Unified access to Coordinator - automatic distribution to optimal Ollama
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
   - Select optimal Agent/Ollama (Load Balancing)
   - Forward request to selected Agent's Ollama via HTTP client

3. **Coordinator → Agent (Internal Communication)**
   ```
   POST http://agent1:11434/api/chat
   Content-Type: application/json

   {"model": "llama2", "messages": [...]}
   ```

4. **Agent → Ollama → Agent (Local Processing)**
   - Agent forwards request to local Ollama instance
   - Ollama processes LLM and generates response

5. **Coordinator → Client (Return Response)**
   ```json
   {
     "message": {"role": "assistant", "content": "..."},
     "done": true
   }
   ```

**From Client's Perspective**:
- Coordinator appears as the only Ollama API server
- No need to be aware of multiple internal Ollama instances
- Complete with a single HTTP request

### Benefits of Proxy Pattern

1. **Unified Endpoint**
   - Clients only need to know the Coordinator URL
   - No need to know each Agent/Ollama location

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
ollama-coordinator/
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
│   │   │   └── proxy.rs    # Ollama proxy
│   │   ├── registry/    # Agent state management
│   │   ├── db/          # Database access
│   │   └── main.rs
│   ├── migrations/      # Database migrations
│   └── Cargo.toml
├── agent/               # Agent application
│   ├── src/
│   │   ├── ollama.rs    # Automatic Ollama management
│   │   ├── client.rs    # Coordinator communication
│   │   ├── metrics.rs   # Metrics collection
│   │   └── main.rs
│   ├── tests/
│   │   └── integration/ # Integration tests
│   └── Cargo.toml
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
   cargo run -p ollama-coordinator-coordinator
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
- **Ollama**: Automatically downloaded and installed when not present
  - Progress display during download
  - Automatic retry on network errors
  - SHA256 checksum verification
  - Proxy support (HTTP_PROXY, HTTPS_PROXY environment variables)
- **LLM Models**: Automatically downloaded on first startup
  - Memory-based model selection (appropriate model size for available RAM)
  - Real-time progress display with streaming status updates
  - Automatic retry on network errors
  - Models pulled via Ollama API
- **Management**: Browser-based WebUI dashboard for agent settings and monitoring

### Coordinator Setup

```bash
# Clone repository
git clone https://github.com/your-org/ollama-coordinator.git
cd ollama-coordinator

# Build Coordinator
cd coordinator
cargo build --release

# Start Coordinator
./target/release/ollama-coordinator-coordinator
# Default: http://0.0.0.0:8080
```

### Agent Setup

```bash
# Build Agent
cd agent
cargo build --release

# Start Agent
COORDINATOR_URL=http://coordinator-host:8080 ./target/release/ollama-coordinator-agent

# Or start without environment variable (default: http://localhost:8080)
./target/release/ollama-coordinator-agent
```

**Note**: Ollama is automatically downloaded and installed on first startup if not already present. The agent will:
- Detect the platform (Linux, macOS, Windows)
- Download the appropriate Ollama binary
- Verify integrity with SHA256 checksum
- Install to `~/.ollama-agent/bin/`
- **Automatically download LLM models**:
  - Memory-based model selection (chooses appropriate model size)
  - Real-time progress display during model download
  - Automatic retry on network errors (configurable via environment variables)
  - Streaming response processing for live status updates
- Start Ollama and register with the coordinator

Manual installation is also supported. Download Ollama from [ollama.ai](https://ollama.ai).

### Release Automation

Merging changes into `main` runs `.github/workflows/semantic-release.yml`, which derives the next semantic version from Conventional Commits, updates the workspace Cargo manifests and `CHANGELOG.md`, and then triggers `.github/workflows/release-binaries.yml` to build/validate platform archives and attach them to the newly published GitHub Release. The workflow produces platform-specific archives with consistent formats:

- `ollama-coordinator-linux-*` / `ollama-coordinator-macos-*` → `.tar.gz`
- `ollama-coordinator-windows-*` → `.zip`

Releases must include both coordinator and agent binaries together with `README.md`, `README.ja.md`, and `LICENSE` so operators can download a single archive per target environment.

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
OLLAMA_GPU_AVAILABLE=true \
OLLAMA_GPU_MODEL="Your GPU Model" \
OLLAMA_GPU_COUNT=1 \
./target/release/ollama-coordinator-agent
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
   COORDINATOR_URL=http://coordinator:8080 cargo run --release --bin ollama-coordinator-agent

   # Machine 2
   COORDINATOR_URL=http://coordinator:8080 cargo run --release --bin ollama-coordinator-agent

   # Machine 3
   COORDINATOR_URL=http://coordinator:8080 cargo run --release --bin ollama-coordinator-agent
   ```

3. **Use Ollama API Through Coordinator**
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

#### Coordinator
- `COORDINATOR_HOST`: Bind address (default: `0.0.0.0`)
- `COORDINATOR_PORT`: Port number (default: `8080`)
- `DATABASE_URL`: Database URL (default: `sqlite://coordinator.db`)
- `HEALTH_CHECK_INTERVAL`: Health check interval in seconds (default: `30`)
- `AGENT_TIMEOUT`: Agent timeout in seconds (default: `60`)
- `LOAD_BALANCER_MODE`: Load balancing strategy - `metrics` for metrics-based or `auto` for advanced (default: `auto`)

#### Agent
- `COORDINATOR_URL`: Coordinator URL (default: `http://localhost:8080`)
- `OLLAMA_PORT`: Ollama port number (default: `11434`)
- `OLLAMA_GPU_AVAILABLE`: Manual GPU availability flag (optional, auto-detected)
- `OLLAMA_GPU_MODEL`: Manual GPU model name (optional, auto-detected)
- `OLLAMA_GPU_COUNT`: Manual GPU count (optional, auto-detected)
- `OLLAMA_DEFAULT_MODEL`: Default LLM model to download (optional, auto-selected based on memory)
- `OLLAMA_PULL_TIMEOUT_SECS`: Timeout for model download in seconds (optional)
- `OLLAMA_API_BASE`: Custom Ollama API base URL (optional, default: `http://127.0.0.1:11434`)

## Development

### Commit Hooks

Install the JavaScript tooling once per clone to enable Husky-managed commit hooks:

```bash
npm install
```

- Runs the `prepare` script and configures Husky's Git hook directory.
- Adds a `commit-msg` hook that executes `commitlint --edit "$1"` so invalid messages fail locally instead of in CI.
- Use `npm run lint:commits` to lint a range manually (defaults to `origin/main..HEAD`).

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
TEST_COORDINATOR_URL=http://localhost:8080 cargo test --test integration_tests -- --ignored

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

## Request History

Ollama Coordinator automatically logs all requests and responses for debugging,
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
- Linux/macOS: `~/.ollama-coordinator/request_history.json`
- Windows: `%USERPROFILE%\.ollama-coordinator\request_history.json`

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
  "ollama_version": "0.1.0",
  "ollama_port": 11434,
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
    "ollama_version": "0.1.0",
    "ollama_port": 11434,
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

#### POST /api/chat
Proxy endpoint for Ollama Chat API.

**Request/Response:** Conforms to [Ollama Chat API specification](https://github.com/ollama/ollama/blob/main/docs/api.md#generate-a-chat-completion)

#### POST /api/generate
Proxy endpoint for Ollama Generate API.

**Request/Response:** Conforms to [Ollama Generate API specification](https://github.com/ollama/ollama/blob/main/docs/api.md#generate-a-completion)

## License

MIT License

## Contributing

Issues and Pull Requests are welcome.

For detailed development guidelines, see [CLAUDE.md](./CLAUDE.md).
