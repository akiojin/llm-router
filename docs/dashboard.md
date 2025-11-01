# Dashboard Guide

This document describes the Ollama Coordinator dashboard, included in the `feature/new-feature` branch. It explains how to run the dashboard, what data is displayed, and how to customize the behaviour for integrators.

---

## 1. Overview

The dashboard is a lightweight HTML/JavaScript UI served directly from the coordinator process. It exposes the following capabilities:

| Area                | Description                                                                           |
|---------------------|---------------------------------------------------------------------------------------|
| System statistics   | Online/offline agent counts, request totals, average response latency                 |
| Agent list          | Filter, search, sort, select, inspect, disconnect, delete, update metadata            |
| Request history     | Live chart of successful/failed requests during the last 60 minutes                   |
| Export              | Download the currently filtered agents as JSON or CSV                                |

The dashboard is implemented with vanilla JavaScript and Chart.js to keep dependencies and build steps minimal.

---

## 2. Running the dashboard

1. Build and start the coordinator (inside the Docker container or on the host):

   ```bash
   cargo run -p ollama-coordinator-coordinator
   ```

2. The coordinator listens on `0.0.0.0:8080` by default. Visit the dashboard in a browser:

   ```
   http://localhost:8080/dashboard
   ```

3. While the process is running, the dashboard automatically refreshes every 5 seconds. If no agents are registered, the list displays helper messages.

### Docker quickstart

If you use the provided compose file:

```bash
docker compose up --build -d
docker compose exec ollama-coordinator cargo run -p ollama-coordinator-coordinator
```

Expose the port as configured (`-p 8080:8080`) and open the dashboard from the host browser.

---

## 3. Working with agents

| Action                 | How to perform                                                                                          |
|------------------------|----------------------------------------------------------------------------------------------------------|
| Filter by status       | Use the “状態” dropdown (すべて / オンライン / オフライン)                                               |
| Search by machine/IP   | Enter a term in the “検索” input (matches machine name, custom label, IP address)                        |
| Sort                   | Click any sortable column header (name, IP, status, uptime, total requests). Click again to reverse.     |
| Select agents          | Use the checkbox next to each row or “すべて選択” in the header.                                         |
| Inspect / edit         | Click “詳細” to open the modal. You can edit display name, tags, notes and press 保存 to persist.        |
| Force disconnect       | In the modal, click “強制切断” to set status offline immediately.                                        |
| Delete agent           | In the modal, click “削除” and confirm. The agent is removed from memory and storage.                     |
| Export list            | Use “JSONエクスポート” or “CSVエクスポート” buttons to download the filtered list.                        |
| View CPU/memory/GPU trend | Open “詳細” and scroll to the “メトリクス” section for CPU/メモリ/GPUの折れ線グラフ (latest 120 samples). |
| Monitor GPU utilisation | The stats card “平均GPU使用率” displays the current average GPU utilisation and memory usage across all fresh agents. |
| Override Ollama download | Set `OLLAMA_DOWNLOAD_URL` (and optionally `OLLAMA_PLATFORM` such as `linux-arm64`) when running the agent to force a specific binary. |
| Auto model preload   | Agents now pull `gpt-oss:20b` automatically once Ollama is up. Override with `OLLAMA_DEFAULT_MODEL`; adjust the pull timeout with `OLLAMA_PULL_TIMEOUT_SECS`. |
| View loaded models    | The “モデル” column shows each agent’s reported models (top entry + preview). Open “詳細” to see the full, deduplicated list. |
| Run multiple agents on one host | Launch each agent with a unique `OLLAMA_PORT` (e.g. 11434, 12434…). The coordinator now treats ports on the same machine as distinct agents. |

Pagination is shown once the list exceeds 50 entries; use the arrows below the table to move between pages.

---

## 4. API reference

The dashboard calls these coordinator endpoints:

| Method | Path                                 | Purpose                                 |
|--------|--------------------------------------|-----------------------------------------|
| GET    | `/api/dashboard/overview`            | Combined agents + stats + history + generation metrics |
| GET    | `/api/dashboard/agents`              | Agent list with current runtime metrics |
| GET    | `/api/dashboard/stats`               | Global coordinator summary              |
| GET    | `/api/dashboard/request-history`     | Recent request history (60 points)      |
| GET    | `/api/dashboard/metrics/:agent_id`   | CPU/memory/GPU history for a specific agent |
| PUT    | `/api/agents/:id/settings`           | Update custom name, tags, notes         |
| DELETE | `/api/agents/:id`                    | Remove agent registration               |
| POST   | `/api/agents/:id/disconnect`         | Force the agent offline                 |

All endpoints return JSON. Payload examples can be found inside `coordinator/src/api/agent.rs`.

---

## 5. Customisation and extension

The front-end source lives in `coordinator/src/web/static/`. Key files:

| File                   | Contents                                                  |
|------------------------|-----------------------------------------------------------|
| `index.html`           | Static layout, modal definitions, buttons                 |
| `styles.css`           | Styling variables (supports light/dark schemes)           |
| `app.js`               | Fetch routines, rendering logic, chart and pagination     |

To customise:

1. Modify the HTML and CSS as desired (no bundler required).
2. Add new endpoints to `coordinator/src/api` and update `app.js` to consume them.
3. Remember to run `cargo fmt` and `cargo test -p ollama-coordinator-coordinator` before committing.
---

## 6. Troubleshooting

- **Dashboard shows “データ取得中…” indefinitely**: Ensure the coordinator process is running and port 8080 is reachable. Check container port mapping (`docker compose ps`).
- **JSON exports include stale data**: Filters are applied per page; ensure you’re exporting after applying the desired filter.
- **High latency or many agents**: Adjust `state.pageSize` in `app.js` or implement lazy loading.
- **Performance indicator shows warning / legacy**: “legacy” means the combined overview endpoint was unavailable and the client fell back to three legacy requests. Warnings appear when fetch (>2s), render (>100ms), or server aggregation (>100ms) exceed the performance budget; investigate coordinator load or reduce agent volume.
- **Metrics chart is empty**: The agent has not yet reported heartbeat metrics. Once the agent sends CPU/メモリ/GPU情報 (`record_metrics`), up to 360 snapshots are retained server-side and the most recent 120 are graphed in the modal.
- **GPU values stay at 0%**: Ensure the agent process can access NVIDIA NVML (the `libnvidia-ml` shared library). If unavailable, GPUメトリクスは自動的に省略されます。

---

## 7. Related specifications

- `SPEC-94621a1f`: Agent registration & heartbeats
- `SPEC-63acef08`: API proxy & request routing
- `SPEC-443acc8c`: Health monitoring
- `SPEC-712c20cf`: Dashboard requirements (current document)
