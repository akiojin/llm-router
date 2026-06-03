# Authentication

llmlb always requires authentication. There is no anonymous / no-auth mode: protected APIs reject
unauthenticated requests with `401 Unauthorized`. On first boot llmlb provisions an admin account
(from `ADMIN_PASSWORD` or an interactive prompt; debug builds also accept `admin` / `test`).

llmlb uses two authentication mechanisms:

1. **JWT** for the dashboard and management APIs (`/api/*`).
   - `/api/dashboard/*` is **JWT-only** (API keys are rejected).
2. **API keys** for the OpenAI-compatible API (`/v1/*`) and selected `/api/*` endpoints for
   ops automation.

The canonical API list lives in `README.md` / `README.ja.md`.

## JWT (dashboard + management)

- `POST /api/auth/login`
- `POST /api/auth/logout`
- `GET /api/auth/me`

Accepted transports:

- `Authorization: Bearer <jwt>`
- HttpOnly cookie `llmlb_jwt` (used by the dashboard)

### CSRF (cookie auth only)

When authenticating via cookies and making mutating requests (POST/PUT/PATCH/DELETE):

- send `X-CSRF-Token: <token>` where `<token>` is the value of the `llmlb_csrf` cookie
- `Origin`/`Referer` must match the dashboard origin

If you authenticate via `Authorization` header, CSRF checks are not applied.

## API keys

Clients send the key via:

- `Authorization: Bearer <api_key>`
- `X-API-Key: <api_key>`

### Permissions

API keys carry a list of `permissions` (string IDs). The backend default-denies: keys without
permissions cannot access protected endpoints. The legacy `scopes` field is deprecated and is
rejected by the API.

| Permission | Grants |
|---|---|
| `openai.inference` | OpenAI-compatible inference endpoints (`POST /v1/*` except `GET /v1/models*`) |
| `openai.models.read` | Model discovery (`GET /v1/models`, `GET /v1/models/:model_id`) |
| `endpoints.read` | Read-only endpoint APIs (`GET /api/endpoints*`) |
| `endpoints.manage` | Endpoint mutations (`POST/PUT/DELETE /api/endpoints*`, `POST /api/endpoints/:id/test`, `POST /api/endpoints/:id/sync`, `POST /api/endpoints/:id/download`) |
| `users.manage` | User management (`/api/users*`) |
| `invitations.manage` | Invitation management (`/api/invitations*`) |
| `models.manage` | Model register/delete (`POST /api/models/register`, `DELETE /api/models/*`) |
| `registry.read` | Model registry access (`GET /api/models/registry/*`) and model list endpoints (`GET /api/models`, `GET /api/models/hub`) |
| `logs.read` | Node log proxy (`GET /api/nodes/:node_id/logs`) |
| `metrics.read` | Metrics export (`GET /api/metrics/cloud`) |

### Self-service API key management

Authenticated users manage their own keys via JWT-only endpoints:

- `GET /api/me/api-keys`
- `POST /api/me/api-keys`
- `PUT /api/me/api-keys/:id`
- `DELETE /api/me/api-keys/:id`

`POST /api/me/api-keys` is server-managed and always issues keys with:

- `openai.inference`
- `openai.models.read`

Note: For routes that accept either JWT or API key, llmlb prefers JWT if present. Avoid sending a
JWT-looking token (three dot-separated segments) as an API key in `Authorization: Bearer`.

### Debug keys (debug builds only)

In debug builds (`#[cfg(debug_assertions)]`) the following API keys are accepted:

- `sk_debug`: all permissions
- `sk_debug_admin`: all permissions
- `sk_debug_api`: `openai.inference` + `openai.models.read`
- `sk_debug_runtime`: `registry.read`
