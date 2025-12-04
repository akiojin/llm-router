# Quickstart: GPU必須ノード登録チェックリスト

1. **Agentを起動する**

   GPU搭載マシンで `OLLAMA_GPU_AVAILABLE` などの環境変数を設定せずに agent を起動する。ログに `GPU Detected` が出力され、登録リクエストに `gpu_devices` が含まれていることを確認する。

2. **登録APIを検証する**

   `POST /api/agents` へ次のJSONを送信する。

   ```json
   {
     "machine_name": "gpu-node-1",
     "ip_address": "10.0.0.10",
     "runtime_version": "0.1.30",
     "runtime_port": 11434,
     "gpu_available": true,
     "gpu_devices": [
       { "model": "NVIDIA RTX 4090", "count": 2 }
     ]
   }
   ```

   成功時は `status: "registered"` が返り、`GET /api/agents` のレスポンスに `gpu_devices` が含まれる。`gpu_devices: []` などGPU情報が欠損したリクエストは 403 と `{"error":"検証エラー: GPU hardware is required"}` を返す。

3. **ストレージクリーンアップを確認する**

   `coordinator/tests/support/fixtures/agents/gpu_missing.json` を `agents.json` として配置し、Coordinator を起動する。起動ログに `Removing GPU-less agent` が表示され、`agents.json` からGPU無しレコードが削除されることを確認する。

4. **ダッシュボードの表示を確認する**

   `/dashboard/` を開き、テーブルとモーダルで `GPU NVIDIA RTX 4090 (2枚)` のようにモデル名と枚数が表示されることを確認する。また `GET /api/dashboard/agents` のレスポンスにも `gpu_devices` 配列が含まれることを確認する。

5. **ローカル検証を実行する**

   `cargo fmt` → `cargo clippy -- -D warnings` → `cargo test`
