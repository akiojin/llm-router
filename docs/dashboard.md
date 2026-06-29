# Dashboard

llmlb serves the admin dashboard UI as a React SPA.

- Dashboard shell: `GET /dashboard`
- Dashboard static assets: `GET /dashboard/*`
- Endpoint Playground route: `/dashboard/#playground/:endpointId`
- LB Playground route: `/dashboard/#lb-playground`

## Playground modes

### Endpoint Playground

- Route: `#playground/:endpointId`
- Purpose: direct endpoint verification
- API: `POST /api/endpoints/:id/chat/completions` (JWT only)

### LB Playground

- Route: `#lb-playground`
- Purpose: load balancer routing and distribution validation
- Modes:
  - Chat: available to all authenticated users.
  - Load Test: **admin role only**. It issues many real inference
    requests through the load balancer, so viewers cannot run it
    (hidden in the UI and rejected with 403 by the backend).
- APIs:
  - `GET /api/dashboard/playground/models` (JWT/session)
  - `POST /api/dashboard/playground/chat/completions`
    (Chat; JWT/session + CSRF)
  - `POST /api/dashboard/playground/load-test/chat/completions`
    (Load Test; JWT/session + CSRF + admin role)
  - `GET /api/dashboard/request-responses`
    (JWT only, for distribution aggregation)

## Dashboard APIs used by UI

- `GET /api/dashboard/overview`
- `GET /api/dashboard/stats`
- `GET /api/dashboard/endpoints`
- `GET /api/dashboard/metrics/:node_id`
- `GET /api/dashboard/request-history`
- `GET /api/dashboard/request-responses`
- `GET /api/dashboard/request-responses/:id`
- `GET /api/dashboard/request-responses/export`
- `GET /api/dashboard/logs/lb`
- `GET /api/dashboard/stats/tokens`
- `GET /api/dashboard/stats/tokens/daily`
- `GET /api/dashboard/stats/tokens/monthly`

## Build (regenerate embedded assets)

```bash
pnpm install
pnpm --filter @llmlb/dashboard build
```

This regenerates embedded static assets under `llmlb/src/web/static/`.
