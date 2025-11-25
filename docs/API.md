# API 補足: クラウドモデルプレフィックス

## 対象エンドポイント

- `POST /v1/chat/completions`
- `POST /v1/completions`
- `POST /v1/embeddings`

## モデル指定ルール

| プレフィックス | 転送先 | 例 |
| --- | --- | --- |
| `openai:` | OpenAI API (`OPENAI_BASE_URL`, 既定 `https://api.openai.com`) | `openai:gpt-4o` |
| `google:` | Google Generative Language API (`GOOGLE_API_BASE_URL`, 既定 `https://generativelanguage.googleapis.com/v1beta`) | `google:gemini-pro` |
| `anthropic:` (`ahtnorpic:` 可) | Anthropic API (`ANTHROPIC_API_BASE_URL`, 既定 `https://api.anthropic.com`) | `anthropic:claude-3-opus` |

プレフィックスは転送前に除去され、クラウド側にはプレフィックスなしのモデル名が送信されます。プレフィックスなしのモデルは従来どおりローカルLLMへルーティングされます。

## 必須環境変数

- `OPENAI_API_KEY`
- `GOOGLE_API_KEY`
- `ANTHROPIC_API_KEY`

任意: `OPENAI_BASE_URL`, `GOOGLE_API_BASE_URL`, `ANTHROPIC_API_BASE_URL`

## ストリーミング

`stream: true` を指定するとクラウドAPIのストリーミング(SSE/チャンク)をそのままパススルーします。

## メトリクス

- エンドポイント: `/metrics/cloud`（Prometheus text）
- 指標:
  - `cloud_requests_total{provider,status}`
  - `cloud_request_latency_seconds{provider}`

## エラーハンドリングの方針

| ケース | ステータス | ボディ概要 |
| --- | --- | --- |
| APIキー未設定 | 401 Unauthorized | `error: "<PROVIDER>_API_KEY is required for ..."` |
| 不明/未実装プレフィックス | 400 Bad Request | `error: "unsupported cloud provider prefix"` |
| クラウド側4xx/5xx | クラウドと同じ | クラウドレスポンスをそのまま返却（JSON/SSEヘッダ維持） |
