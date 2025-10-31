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
- **Self-registering Agents**: Agents automatically register with the Coordinator
- **WebUI Management**: Manage agent settings, monitoring, and control through browser-based dashboard
- **Cross-Platform Support**: Works on Windows 10+, macOS 12+, and Linux

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

## Installation

### Requirements

- **Coordinator**: Linux / Windows 10+ / macOS 12+, Rust 1.70+
- **Agent**: Windows 10+ / macOS 12+ (CLI-based application), Rust 1.70+
- **Ollama**: Pre-installation recommended (automatic download is a future enhancement)
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

**Note**: Ensure Ollama is installed and running on the agent machine before starting the agent. Download Ollama from [ollama.ai](https://ollama.ai).

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

#### Agent
- `COORDINATOR_URL`: Coordinator URL (default: `http://localhost:8080`)
- `OLLAMA_PORT`: Ollama port number (default: `11434`)

## Development

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
```

### Spec-Driven Development

This project follows Spec-Driven Development:

1. `/speckit.specify` - Create feature specification
2. `/speckit.plan` - Create implementation plan
3. `/speckit.tasks` - Break down into tasks
4. Execute tasks (strict TDD cycle)

See [CLAUDE.md](./CLAUDE.md) for details.

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
  "ollama_port": 11434
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
    "last_seen": "2025-10-30T12:05:00Z"
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
