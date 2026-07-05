//! 人間可読な API ガイドテキスト生成（GuideCategory 単位）
//!
//! arch-review [H6] round2: cli/assistant.rs からガイド生成を分離。

pub(super) fn overview_guide(router_url: &str) -> String {
    format!(
        "# llmlb API Overview\n\n## Base URL\n\n```\n{router_url}\n```\n\n## API Categories\n\n| Category | Base Path | Notes |\n|----------|-----------|-------|\n| OpenAI-Compatible | /v1/* | Inference APIs. Requires an API key with `api` scope. |\n| Management | /api/* | Endpoint/model/dashboard/admin APIs. Prefer an API key with `admin` scope. |\n| Dashboard UI | /dashboard | Browser UI. Uses HttpOnly cookies after login. |\n\n## Authentication\n\n### API Key Authentication (recommended for programmatic access)\n\n**Header**: `X-API-Key: sk_xxx` (or `Authorization: Bearer sk_xxx`)\n\nScopes (examples):\n- `api`: /v1/* inference endpoints\n- `admin`: /api/* management endpoints\n\nThis CLI can auto-inject:\n- `LLMLB_API_KEY` for /v1/*\n- `LLMLB_ADMIN_API_KEY` for /api/* (preferred)\n\n### Dashboard Session (browser UI)\n\nThe dashboard uses **HttpOnly cookies** for JWT sessions. This CLI does not manage browser cookies.\nUse scoped API keys for automation."
    )
}

pub(super) fn openai_guide(router_url: &str) -> String {
    format!(
        "# OpenAI-Compatible API (/v1/*)\n\n## Chat Completions\n\n**Endpoint**: POST {router_url}/v1/chat/completions\n\n```bash\ncurl -X POST {router_url}/v1/chat/completions \\\n  -H \"Content-Type: application/json\" \\\n  -H \"X-API-Key: YOUR_API_KEY\" \\\n  -d '{{\n    \"model\": \"llama3.2:3b\",\n    \"messages\": [\n      {{\"role\": \"system\", \"content\": \"You are a helpful assistant.\"}},\n      {{\"role\": \"user\", \"content\": \"Hello!\"}}\n    ],\n    \"stream\": false\n  }}'\n```\n\n## Cloud Routing (model prefix)\n\n```bash\ncurl -X POST {router_url}/v1/chat/completions \\\n  -H \"Content-Type: application/json\" \\\n  -H \"X-API-Key: YOUR_API_KEY\" \\\n  -d '{{\"model\": \"openai:gpt-4o\", \"messages\": [{{\"role\":\"user\",\"content\":\"Hello\"}}]}}'\n```\n\nSupported prefixes:\n- `openai:`\n- `google:`\n- `anthropic:`\n\n## List Models\n\n**Endpoint**: GET {router_url}/v1/models\n\n```bash\ncurl {router_url}/v1/models \\\n  -H \"X-API-Key: YOUR_API_KEY\"\n```\n\n## Embeddings\n\n**Endpoint**: POST {router_url}/v1/embeddings\n\n```bash\ncurl -X POST {router_url}/v1/embeddings \\\n  -H \"Content-Type: application/json\" \\\n  -H \"X-API-Key: YOUR_API_KEY\" \\\n  -d '{{\"model\": \"nomic-embed-text-v1.5\", \"input\": \"Hello world\"}}'\n```"
    )
}

pub(super) fn endpoint_management_guide(router_url: &str) -> String {
    format!(
        "# Endpoint Management API (/api/endpoints)\n\n## List Endpoints\n\n**Endpoint**: GET {router_url}/api/endpoints\n\n```bash\ncurl {router_url}/api/endpoints \\\n  -H \"X-API-Key: ADMIN_API_KEY\"\n```\n\n## Create Endpoint\n\n**Endpoint**: POST {router_url}/api/endpoints\n\n```bash\ncurl -X POST {router_url}/api/endpoints \\\n  -H \"Content-Type: application/json\" \\\n  -H \"X-API-Key: ADMIN_API_KEY\" \\\n  -d '{{\n    \"name\": \"xllm-local\",\n    \"base_url\": \"http://127.0.0.1:8080\",\n    \"api_key\": null\n  }}'\n```\n\nNotes:\n- `endpoint_type` can be provided to override auto-detection (xllm/ollama/vllm/openai_compatible).\n- If omitted, llmlb will auto-detect the endpoint type (when reachable)."
    )
}

pub(super) fn model_management_guide(router_url: &str) -> String {
    format!(
        "# Model Management API (/api/models/*)\n\n## List Models (management view)\n\n**Endpoint**: GET {router_url}/api/models/hub\n\n```bash\ncurl {router_url}/api/models/hub \\\n  -H \"X-API-Key: ADMIN_API_KEY\"\n```\n\n## Register Model (admin only)\n\n**Endpoint**: POST {router_url}/api/models/register\n\n```bash\ncurl -X POST {router_url}/api/models/register \\\n  -H \"Content-Type: application/json\" \\\n  -H \"X-API-Key: ADMIN_API_KEY\" \\\n  -d '{{\n    \"repo\": \"TheBloke/Llama-2-7B-GGUF\",\n    \"filename\": \"llama-2-7b.Q4_K_M.gguf\"\n  }}'\n```\n\n## Delete Model (admin only)\n\n**Endpoint**: DELETE {router_url}/api/models/:model_name\n\n```bash\ncurl -X DELETE {router_url}/api/models/gpt-oss-20b \\\n  -H \"X-API-Key: ADMIN_API_KEY\"\n```\n\nNote:\n- llmlb does not push binaries to runtimes. Runtimes fetch manifests and artifacts as needed."
    )
}

pub(super) fn dashboard_guide(router_url: &str) -> String {
    format!(
        "# Dashboard API (/api/dashboard/*)\n\n## Overview\n\n**Endpoint**: GET {router_url}/api/dashboard/overview\n\n```bash\ncurl {router_url}/api/dashboard/overview \\\n  -H \"X-API-Key: ADMIN_API_KEY\"\n```\n\n## Stats\n\n**Endpoint**: GET {router_url}/api/dashboard/stats\n\n```bash\ncurl {router_url}/api/dashboard/stats \\\n  -H \"X-API-Key: ADMIN_API_KEY\"\n```\n\n## Request/Response History (API)\n\n**Endpoint**: GET {router_url}/api/dashboard/request-responses\n\n```bash\ncurl {router_url}/api/dashboard/request-responses \\\n  -H \"X-API-Key: ADMIN_API_KEY\"\n```\n\n## Router Logs\n\n**Endpoint**: GET {router_url}/api/dashboard/logs/lb\n\n```bash\ncurl {router_url}/api/dashboard/logs/lb \\\n  -H \"X-API-Key: ADMIN_API_KEY\"\n```"
    )
}
