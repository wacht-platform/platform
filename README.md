<h1 align="center">
  <a href="https://wacht.dev" style="text-decoration:none;">Wacht Platform API</a>
</h1>

<p align="center">Core backend services for Wacht: auth APIs, gateway authorization, OAuth flows, realtime notifications, async workers, and billing/usage pipelines.</p>

<p align="center">
  <a href="https://wacht.dev">Website</a> |
  <a href="https://docs.wacht.dev">Documentation</a> |
  <a href="https://github.com/wacht-platform/platform-api/issues">Issues</a>
</p>

## What this repository contains

This workspace runs the control-plane and runtime backend for Wacht.

- API services for console, frontend/backend integration, OAuth, gateway authz, and realtime delivery
- Background workers for webhook delivery, retries, notifications, billing processing, and agent tasks
- Shared command/query/data crates used by all services
- OAuth relay service for hosted OAuth callback relay scenarios

## Service map

| Service | Binary | Default port |
| --- | --- | --- |
| Console API | `console-api` | `3001` (`CONSOLE_API_PORT`) |
| Backend API | `backend-api` | `3001` (`BACKEND_API_PORT`) |
| OAuth API | `oauth-api` | `3002` (`OAUTH_API_PORT`) |
| Gateway API | `gateway-api` | `3002` |
| Realtime API | `realtime-api` | `3002` |
| Worker | `worker` | N/A (NATS consumer) |

Notes:
- Several binaries share the same default port. Run each service on distinct ports in local development.
- Worker does not expose an HTTP port; it consumes background jobs from NATS/JetStream.

## Workspace layout

- `platform/` - API binaries and routing/application handlers
- `worker/` - async consumers, schedulers, and task processors
- `agent-engine/` - agent execution runtime
- `commands/` - write operations (mutations)
- `queries/` - read operations
- `models/` - data model layer
- `dto/` - shared API DTOs
- `common/` - shared state/services (DB, Redis, NATS, storage, integrations)
- `oauth-relay/` - Cloudflare Worker relay crate

## Runtime dependencies

- PostgreSQL
- Redis
- NATS (JetStream)
- S3-compatible object storage (R2)
- ClickHouse
- Cloudflare API access (custom hostname + DNS verification flows)
- Postmark credentials (email delivery flows)

## Environment variables

`AppState::new_from_env()` requires these variables:

- `DATABASE_PRIMARY_PRIVATE` or `DATABASE_PRIMARY_PUBLIC`
- `REDIS_URL`
- `NATS_HOST`
- `R2_ENDPOINT`
- `R2_ACCESS_KEY_ID`
- `R2_SECRET_ACCESS_KEY`
- `CLOUDFLARE_API_KEY`
- `CLOUDFLARE_ZONE_ID`
- `POSTMARK_ACCOUNT_TOKEN`
- `POSTMARK_SERVER_TOKEN`
- `CLICKHOUSE_HOST`
- `CLICKHOUSE_PASSWORD`
- `ENCRYPTION_KEY`

Optional:

- `USE_PUBLIC_NETWORK` (switches DB URL selection)
- `AGENT_STORAGE_GATEWAY_URL`
- `AGENT_STORAGE_ACCESS_KEY`
- `AGENT_STORAGE_SECRET_KEY`
- `CONSOLE_DEPLOYMENT_ID` (used by selected worker tasks)

## Run locally

From this directory, in separate terminals:

```bash
# Console API
CONSOLE_API_PORT=3001 cargo run -p platform --bin console-api

# Backend API
BACKEND_API_PORT=3003 cargo run -p platform --bin backend-api

# OAuth API
OAUTH_API_PORT=3002 cargo run -p platform --bin oauth-api

# Gateway API
cargo run -p platform --bin gateway-api

# Realtime API
cargo run -p platform --bin realtime-api

# Worker
cargo run -p platform-worker --bin worker
```

## Build and checks

```bash
cargo check --workspace
cargo build --workspace
```

## Docker

A workspace Dockerfile is included at [`Dockerfile`](./Dockerfile).

- Build stage compiles all binaries in release mode.
- Runtime image expects command selection externally (`backend`, `console`, `oauth-api`, `realtime`, `gateway`, `worker`) via entrypoint.

## Support

- Report issues: [GitHub Issues](https://github.com/wacht-platform/platform-api/issues)
- Product docs: [docs.wacht.dev](https://docs.wacht.dev)

## License

GNU Affero General Public License v3.0 (AGPL-3.0-only). See [LICENSE.md](./LICENSE.md).
