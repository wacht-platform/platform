<h1 align="center">
  <a href="https://wacht.dev" style="text-decoration:none;">Wacht Platform API</a>
</h1>

<p align="center">
  <strong>The open-source identity, access, and agent runtime backend for modern SaaS.</strong>
</p>

<p align="center">
  <a href="https://github.com/wacht-platform/platform-api/stargazers">
    <img alt="GitHub Stars" src="https://img.shields.io/github/stars/wacht-platform/platform-api?style=flat-square" />
  </a>
  <a href="https://github.com/wacht-platform/platform-api/blob/main/LICENSE.md">
    <img alt="License" src="https://img.shields.io/badge/license-AGPL--3.0-blue?style=flat-square" />
  </a>
  <img alt="Status" src="https://img.shields.io/badge/status-public%20beta-blue?style=flat-square" />
  <img alt="Built with Rust" src="https://img.shields.io/badge/built%20with-rust-orange?style=flat-square" />
</p>

<p align="center">
  <a href="https://wacht.dev">Website</a> ·
  <a href="https://wacht.dev/docs">Docs</a> ·
  <a href="https://github.com/wacht-platform/platform-api/issues">Issues</a> ·
  <a href="https://wacht.dev/changelog">Changelog</a>
</p>

---

## Overview

Wacht Platform API is the backend that powers [Wacht](https://wacht.dev) — a programmable
identity, access, and agent platform for B2B and SaaS products. It exposes the control plane,
authentication runtime, integration surface, and the agent execution engine that applications
build on top of.

It is designed for teams that treat identity, authorization, and AI workflows as first-class
product surfaces rather than commodity infrastructure.

## Capabilities

- **Multi-tenant authentication.** Sign-in, sign-up, MFA, sessions, and deployment-scoped policies.
- **B2B access model.** Organizations, workspaces, roles, and a permission catalog suitable for
  customer-facing admin UIs.
- **Machine and user authorization.** Token issuance, API keys, scoped credentials, and
  authorization decisions for both human and service callers.
- **OAuth and integrations.** First-party OAuth provider plus relay flows for external services.
- **Event-driven operations.** Webhooks, async workers, retries, notifications, usage metering,
  and billing hooks.
- **Agent runtime.** A first-class engine for long-running, tool-using agents with sandboxed
  execution, scheduled work, and human-in-the-loop approvals.

## Architecture

The system is organized into four planes:

| Plane          | Responsibility                                                            |
| -------------- | ------------------------------------------------------------------------- |
| Control plane  | Configure deployments, policies, auth factors, and access models.         |
| Runtime plane  | Execute sign-in/sign-up flows and authorization decisions at request time.|
| Integration    | OAuth providers, webhooks, and external service connectors.               |
| Operations     | Background workers, retries, notifications, usage and billing pipelines.  |

This repository is the backend for those four planes. It is consumed by:

- **`console`** — operator UI for managing platform configuration.
- **`frontend-api`** — end-user authentication flows embedded in application frontends.

## Repository Layout

```
platform/        HTTP entrypoints (console-api, frontend-api, oauth-relay)
agent-engine/    Agent execution runtime, planner, tool dispatch
worker/          Background job runner (webhooks, retries, schedules)
commands/        Write-side handlers (CQRS-style commands)
queries/         Read-side projections and query handlers
models/          Domain models and persistence types
dto/             Wire types shared across services
templatekit/     Prompt and template assets for the agent engine
common/          Shared utilities (telemetry, error, config)
oauth-relay/     OAuth relay service
scripts/         Operational and developer scripts
```

## Quickstart

Requirements: a recent stable Rust toolchain, PostgreSQL, and NATS.

```bash
# Verify the workspace builds
cargo check --workspace

# Run the console API
CONSOLE_API_PORT=3001 cargo run -p platform --bin console-api

# Run the background worker
cargo run -p platform-worker --bin worker
```

See the [documentation](https://wacht.dev/docs) for environment variables, schema migrations,
and deployment guidance.

## Status

Wacht Platform API is in **public beta**. The HTTP surface and data model are stabilizing;
breaking changes are documented in the changelog. Production usage is supported with the
expectations typical of a beta release.

## Contributing

We're not accepting pull requests yet — the contribution process isn't set up. Forks,
self-hosting, and any other use the AGPL-3.0 allows are welcome. Self-hosting documentation
is still in progress.

## Support

- Product and integration docs: [wacht.dev/docs](https://wacht.dev/docs).
- Direct assistance: [engineering@intellinesia.com](mailto:engineering@intellinesia.com).

## License

Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0-only).
See [LICENSE.md](./LICENSE.md) for the full text.
