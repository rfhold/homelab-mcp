# Architecture Overview

## Status

The repository implements hosted OAuth, generic OIDC, PostgreSQL persistence, authenticated MCP, and four bounded Grafana query actions.

The container and pipeline declarations can build the runtime, but it has not been deployed. Preview still runs the prior health-only image, and production remains excluded.

## Purpose

`homelab-mcp` exposes bounded homelab integrations through MCP. Grafana query access is the first integration; the service does not expose Grafana credentials or direct datasource access to clients.

## Service Boundary

### Runtime

`src/main.rs` is the composition root for a Rust 1.96, edition 2024 Axum process. It loads concern-specific configuration, builds the concrete `Services` collection, initializes hosted OAuth, creates the MCP handler, and binds `0.0.0.0:14333`.

`src/lib.rs` exposes the application modules to the binary and future integration tests. `src/services.rs` owns configured integration services. Each integration owns its client lifecycle, actions, private errors, telemetry, and upstream translation under `src/integrations/`; MCP handlers depend on `Services`, not third-party clients or application configuration.

`GET /health` returns unconditional process health. `GET /ready` performs bounded live PostgreSQL and signing-key-readiness checks. It does not probe Authentik or Grafana.

The router merges generic OAuth and OIDC endpoints with authenticated stateless `/mcp`. Startup applies the generic Kuri migration history in schema `mcp`, including the V4 OIDC attempt table, and initializes the protected signing key.

### MCP Service

The current worktree service:

- uses Kuri's generic private `mcp` crate at a reviewed immutable Git revision;
- serves MCP through Streamable HTTP revision `2026-07-28` at `/mcp`;
- uses `#[mcp::progressive_server]` to generate one read-only progressive tool, `grafana_query`;
- exposes `logql`, `promql`, `traceql`, and `profiles` actions;
- queries Grafana's HTTP API through fixed Loki, Mimir, Tempo, and Pyroscope datasource UIDs;
- enforces local OAuth access tokens before MCP request handling; and
- persists generic OAuth and OIDC state in PostgreSQL schema `mcp`.

The [Grafana Query specifications](../grafana-query/README.md) own query bounds and results. The [access document](access-authentication.md) owns authentication and authorization details.

## Component Status

| Component | Status | Responsibility |
| --- | --- | --- |
| Axum host | Implemented locally | Initialize dependencies, serve health/runtime routes, and handle graceful shutdown. |
| Container declaration | Implemented locally | Build release and runtime images; Rust tests run through Cargo outside the image build. |
| Deployment declarations | Implemented and applied to preview | Supply the runtime variables, Secrets, mounts, identity, database, Grafana, workload, and route. Preview still references the prior image. |
| MCP endpoint | Implemented locally | Negotiate stateless Streamable HTTP and dispatch authenticated tool calls. |
| Hosted OAuth issuer | Implemented locally | Issue local access tokens and manage durable OAuth client and token state. |
| Generic OIDC integration | Implemented locally | Use MCP-owned one-shot OIDC transactions and hosted continuation with Authentik. |
| Grafana integration | Worktree implemented; deployment pending | Own fixed-destination clients, action validation, response normalization, safe error translation, and bounded telemetry for four datasource families. |
| PostgreSQL use | Implemented locally | Store generic OAuth state and encrypted signing material through migrations V1-V3, with one-shot OIDC attempts added by V4. |
| Wrapping-key use | Implemented locally | Load the mounted keyring and protect persisted OAuth signing keys. |

## MCP Request Flow

1. An MCP client discovers the service's hosted OAuth metadata.
2. The service redirects browser authentication to Authentik.
3. The service completes browser authentication and issues its own access token.
4. The client sends the local access token to `/mcp`.
5. The service validates the token and required `mcp:use` scope.
6. The service validates the `grafana_query` action arguments.
7. The service queries Grafana with its Viewer service-account token.
8. The action returns a semantic `McpToolResult`.

## Trust Boundaries

- Authentik authenticates a browser user. It does not issue tokens accepted by `/mcp`.
- The local OAuth issuer authorizes MCP access.
- Grafana receives only server-originated requests with the service-account token.
- Loki remains behind Grafana and has no direct service integration.
- PostgreSQL and the wrapping-key file hold separate parts of durable OAuth protection.

## Excluded Systems

Kuri-specific native APIs, agent sessions, S3 storage, dashboards, and direct Loki access are outside this service boundary.
