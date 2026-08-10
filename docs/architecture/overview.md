# Architecture Overview

## Status

The repository implements hosted OAuth, generic OIDC, PostgreSQL persistence, authenticated MCP, six bounded Grafana reads, and bounded silence creation.

Preview runs the authenticated runtime from commit `4f2e192`. Health, readiness, OAuth metadata, and the unauthenticated MCP Bearer challenge are verified. The worktree alerting revision has not been deployed or operated live. Full browser OAuth, authenticated preview MCP calls, live Grafana behavior, and Editor permission operation remain unverified; production remains excluded.

## Purpose

`homelab-mcp` exposes bounded homelab integrations through MCP. Grafana reads and silence creation are the first integration; the service does not expose Grafana credentials or direct datasource access to clients.

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
- uses `#[mcp::progressive_server]` to generate read-only `grafana_query` and operationally consequential `grafana_exec`;
- exposes six read actions on `grafana_query` and only `create_silence` on `grafana_exec`;
- queries Grafana's HTTP API through fixed Loki, Mimir, Tempo, and Pyroscope datasource UIDs;
- reads Grafana alerting state and creates bounded silences through fixed alerting API routes;
- enforces local OAuth access tokens before MCP request handling; and
- persists generic OAuth and OIDC state in PostgreSQL schema `mcp`.

The [Grafana tool specifications](../grafana-query/README.md) own action inputs, routes, bounds, results, and errors. The [access document](access-authentication.md) owns authentication and authorization details.

## Component Status

| Component | Status | Responsibility |
| --- | --- | --- |
| Axum host | Deployed to preview | Initialize dependencies, serve health/runtime routes, and handle graceful shutdown. |
| Container declaration | Applied to preview | Build release and runtime images; Rust tests run through Cargo outside the image build. |
| Deployment declarations | Previous revision applied; worktree update pending | Supply the runtime variables, Secrets, mounts, identity, database, Grafana, workload, and route. |
| MCP endpoint | Worktree updated; previous revision deployed | Negotiate stateless Streamable HTTP and dispatch authenticated tool calls; the deployed unauthenticated challenge is verified. |
| Hosted OAuth issuer | Deployed to preview | Publish metadata, issue local access tokens, and manage durable OAuth client and token state; metadata is verified. |
| Generic OIDC integration | Deployed, flow unverified | Use MCP-owned one-shot OIDC transactions and hosted continuation with Authentik. |
| Grafana integration | Worktree implemented; deployment pending | Own fixed-destination reads and writes, action validation, response normalization, safe error translation, and bounded telemetry for datasource and alerting APIs. |
| PostgreSQL use | Deployed to preview | Store generic OAuth state and encrypted signing material through migrations V1-V3, with one-shot OIDC attempts added by V4. |
| Wrapping-key use | Deployed to preview | Load the mounted keyring and protect persisted OAuth signing keys. |

## MCP Request Flow

1. An MCP client discovers the service's hosted OAuth metadata.
2. The service redirects browser authentication to Authentik.
3. The service completes browser authentication and issues its own access token.
4. The client sends the local access token to `/mcp`.
5. The service validates the token and required `mcp:use` scope.
6. The service validates the selected `grafana_query` or `grafana_exec` action arguments.
7. The service contacts only the action's fixed Grafana route with its Editor service-account token.
8. The action returns a semantic `McpToolResult`.

## Trust Boundaries

- Authentik authenticates a browser user. It does not issue tokens accepted by `/mcp`.
- The local OAuth issuer authorizes MCP access.
- The single `mcp:use` scope authorizes every read and silence-creation action; there is no narrower mutation scope.
- Grafana receives only server-originated requests with the service-account token.
- Loki remains behind Grafana and has no direct service integration.
- PostgreSQL and the wrapping-key file hold separate parts of durable OAuth protection.

## Excluded Systems

Kuri-specific native APIs, agent sessions, S3 storage, dashboards, and direct Loki access are outside this service boundary.
