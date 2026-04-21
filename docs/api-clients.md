# API Client Examples

Sample calls for the OpenAI-compatible router with cloud prefixes.

## curl
```bash
curl -X POST http://localhost:32768/v1/responses \
  -H "Content-Type: application/json" \
  -d '{
    "model": "openai:gpt-4o",
    "input": "Hello"
  }'
```

## Python (requests)
```python
import requests

payload = {
    "model": "google:gemini-1.5-pro",
    "input": "Say hi in JSON",
}
resp = requests.post("http://localhost:32768/v1/responses", json=payload)
resp.raise_for_status()
print(resp.json())
```

## Node.js (fetch)
```javascript
const body = {
  model: "anthropic:claude-3-opus",
  input: "Give me three bullets",
};

const res = await fetch("http://localhost:32768/v1/responses", {
  method: "POST",
  headers: { "content-type": "application/json" },
  body: JSON.stringify(body),
});

const data = await res.json();
console.log(data);
```

## Claude Code から接続する

llmlb は Anthropic 互換の `/v1/messages` を提供するため、
[Claude Code](https://docs.anthropic.com/en/docs/claude-code) のバックエンドとして利用できます。

以下の 3 つの環境変数を設定してから `claude` を起動してください。

```bash
# PowerShell / bash / zsh 共通の例
export ANTHROPIC_BASE_URL="http://localhost:32768"
export ANTHROPIC_API_KEY="<llmlb で発行した API キー>"   # 例: sk_debug（デバッグビルドのみ）
export ANTHROPIC_MODEL="<エンドポイントに登録済みのモデル>"  # 例: openai/gpt-oss-20b
```

| 変数 | 役割 |
|------|------|
| `ANTHROPIC_BASE_URL` | Claude Code がリクエストを送る先。llmlb のベース URL（スキーム・ホスト・ポートのみ） |
| `ANTHROPIC_API_KEY` | llmlb のダッシュボードで発行した API キー。デバッグビルドでは `sk_debug` が使える |
| `ANTHROPIC_MODEL` | 使用するモデル。canonical 名（HuggingFace レポ ID 形式）を推奨 |

### モデル名の扱い

`/v1/messages` は受信したモデル名をそのまま upstream に転送せず、
`resolve_canonical_any` + `rewrite_payload_model_for_endpoint` によって
エンドポイントが広告するエイリアス名へ自動変換します。

例: Ollama に `gpt-oss:20b` として登録されたモデルに対し
`ANTHROPIC_MODEL=openai/gpt-oss-20b` を指定した場合、
llmlb は upstream に `gpt-oss:20b` として転送します。

この仕組みにより、クライアント（Claude Code 等）は
エンドポイント種別（Ollama / LM Studio / vLLM / xLLM）ごとの
命名差異を意識せずに canonical 名のみで利用できます。

### 認証競合警告が出る場合

Claude Code 起動時に以下の警告が出ることがあります。

```text
⚠ Auth conflict: Both a token (claude.ai) and an API key (ANTHROPIC_API_KEY) are set.
```

llmlb を使う場合は `ANTHROPIC_API_KEY` 側が正です。以下のいずれかで解消してください。

- `claude /logout` で claude.ai のトークンを破棄する
- または、ログイン時に API キー承認のプロンプトで **No** と答え、セッションを `ANTHROPIC_API_KEY` 側に固定する

### 動作確認（curl）

Claude Code を介さず、直接 llmlb に Anthropic 形式で疎通確認する例です。

```bash
curl -sS -X POST "$ANTHROPIC_BASE_URL/v1/messages" \
  -H "x-api-key: $ANTHROPIC_API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -H "content-type: application/json" \
  -d "{\"model\":\"$ANTHROPIC_MODEL\",\"max_tokens\":32,\"messages\":[{\"role\":\"user\",\"content\":\"ping\"}]}"
```

`200 OK` と `"type":"message"` を含む JSON が返れば接続成功です。
