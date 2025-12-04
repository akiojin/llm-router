-- Agents table: 登録されたエージェント情報
CREATE TABLE IF NOT EXISTS agents (
    id TEXT PRIMARY KEY,
    machine_name TEXT NOT NULL,
    ip_address TEXT NOT NULL,
    runtime_version TEXT NOT NULL,
    runtime_port INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('online', 'offline')),
    registered_at TEXT NOT NULL,
    last_seen TEXT NOT NULL
);

-- Health Metrics table: エージェントのヘルスメトリクス履歴
CREATE TABLE IF NOT EXISTS health_metrics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    agent_id TEXT NOT NULL,
    cpu_usage REAL NOT NULL,
    memory_usage REAL NOT NULL,
    active_requests INTEGER NOT NULL,
    total_requests INTEGER NOT NULL,
    timestamp TEXT NOT NULL,
    FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
);

-- Requests table: リクエスト履歴
CREATE TABLE IF NOT EXISTS requests (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending', 'processing', 'completed', 'failed')),
    duration_ms INTEGER,
    created_at TEXT NOT NULL,
    completed_at TEXT,
    FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
);

-- Indexes for performance
CREATE INDEX IF NOT EXISTS idx_agents_status ON agents(status);
CREATE INDEX IF NOT EXISTS idx_agents_last_seen ON agents(last_seen);
CREATE INDEX IF NOT EXISTS idx_health_metrics_agent_id ON health_metrics(agent_id);
CREATE INDEX IF NOT EXISTS idx_health_metrics_timestamp ON health_metrics(timestamp);
CREATE INDEX IF NOT EXISTS idx_requests_agent_id ON requests(agent_id);
CREATE INDEX IF NOT EXISTS idx_requests_status ON requests(status);
CREATE INDEX IF NOT EXISTS idx_requests_created_at ON requests(created_at);
