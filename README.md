<h1 align="center">
  <a href="https://wacht.dev" style="text-decoration:none;">Wacht Platform API</a>
</h1>

<p align="center">
  Open-source identity platform backend for modern SaaS and B2B products.
</p>

<p align="center">
  <a href="https://wacht.dev">Website</a> |
  <a href="https://docs.wacht.dev">Documentation</a> |
  <a href="https://github.com/wacht-platform/platform-api/issues">Issues</a>
</p>

## What This Platform Is

Wacht Platform API is the backend for Wacht: a programmable identity and access platform.

It is built for teams that treat identity as a product surface, not just login plumbing.

## Why Teams Use It

Wacht helps you ship:

- Multi-tenant authentication with deployment-level control
- B2B access models (organizations, workspaces, roles, permission catalogs)
- API authorization for machine and user clients
- OAuth integrations and token-based runtime flows
- Event-driven operations (webhooks, async workflows, usage tracking)

## Platform Model

At a product level, the platform has four layers:

- Control plane: configure deployments, policies, auth factors, and access models
- Runtime plane: execute sign-in/sign-up and authorization decisions
- Integration plane: OAuth, webhooks, and external service connectors
- Operations plane: workers, retries, notifications, usage and billing pipelines

## How This Repo Fits In Wacht

- `platform-api` is the backend control plane + runtime core
- `console` is the operator UI for managing platform configuration
- `frontend-api` serves end-user auth flows for application frontends

## Contributor Notes (Minimal Repo Map)

- `platform/` API entrypoints
- `worker/` background jobs
- `agent-engine/` agent runtime
- `commands/`, `queries/`, `models/`, `dto/`, `common/` shared backend foundation

## Quickstart

```bash
cargo check --workspace
CONSOLE_API_PORT=3001 cargo run -p platform --bin console-api
cargo run -p platform-worker --bin worker
```

## Support

- Report issues: [GitHub Issues](https://github.com/wacht-platform/platform-api/issues)
- Product docs: [docs.wacht.dev](https://docs.wacht.dev)

## License

GNU Affero General Public License v3.0 (AGPL-3.0-only). See [LICENSE.md](./LICENSE.md).
