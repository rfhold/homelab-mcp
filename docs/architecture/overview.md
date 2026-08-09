# Architecture Overview

## Status

The repository implements hosted OAuth, generic OIDC, PostgreSQL persistence, authenticated MCP, and Grafana LogQL. Its 25 Rust tests pass against the exact reviewed Kuri Git pin.

The container and pipeline declarations can build the runtime, but it has not been deployed. Preview still runs the prior health-only image, and production remains excluded.

## Purpose

`homelab-mcp` exposes bounded Grafana LogQL access through MCP. It does not expose Grafana credentials or direct Loki access to clients.

## Service Boundary

### Working-Tree Runtime

`src/main.rs` starts a Rust 1.96, edition 2024 Axum process. It loads configuration, connects to PostgreSQL, initializes hosted OAuth, creates the MCP/Grafana handler, and binds `0.0.0.0:14333`.

`GET /health` returns unconditional process health. `GET /ready` performs bounded live PostgreSQL and signing-key-readiness checks. It does not probe Authentik or Grafana.

The router merges generic OAuth and OIDC endpoints with authenticated stateless `/mcp`. Startup applies the generic Kuri migration history in schema `mcp`, including the V4 OIDC attempt table, and initializes the protected signing key.

### MCP Service

The working-tree service:

- uses Kuri's generic private `mcp` crate at a reviewed immutable Git revision;
- serves MCP through Streamable HTTP revision `2026-07-28` at `/mcp`;
- uses `#[mcp::progressive_server]` to generate one read-only progressive tool, `grafana_exec`;
- exposes one domain action, `logql`;
- queries Grafana's HTTP API through the fixed datasource UID `loki`;
- enforces local OAuth access tokens before MCP request handling; and
- persists generic OAuth and OIDC state in PostgreSQL schema `mcp`.

The [LogQL specification](../grafana-exec/spec/logql.md) owns query bounds and results. The [access document](access-authentication.md) owns authentication and authorization details.

## Component Status

| Component | Status | Responsibility |
| --- | --- | --- |
| Axum host | Implemented locally | Initialize dependencies, serve health/runtime routes, and handle graceful shutdown. |
| Container declaration | Implemented locally | Build release and runtime images; Rust tests run through Cargo outside the image build. |
| Deployment declarations | Implemented and applied to preview | Supply the runtime variables, Secrets, mounts, identity, database, Grafana, workload, and route. Preview still references the prior image. |
| MCP endpoint | Implemented locally | Negotiate stateless Streamable HTTP and dispatch authenticated tool calls. |
| Hosted OAuth issuer | Implemented locally | Issue local access tokens and manage durable OAuth client and token state. |
| Generic OIDC integration | Implemented locally | Use MCP-owned one-shot OIDC transactions and hosted continuation with Authentik. |
| Grafana adapter | Implemented locally | Send bounded instant or range queries through Grafana's Loki datasource proxy. |
| PostgreSQL use | Implemented locally | Store generic OAuth state and encrypted signing material through migrations V1-V3, with one-shot OIDC attempts added by V4. |
| Wrapping-key use | Implemented locally | Load the mounted keyring and protect persisted OAuth signing keys. |

## MCP Request Flow

1. An MCP client discovers the service's hosted OAuth metadata.
2. The service redirects browser authentication to Authentik.
3. The service completes browser authentication and issues its own access token.
4. The client sends the local access token to `/mcp`.
5. The service validates the token and required `mcp:use` scope.
6. The service validates the `grafana_exec` and `logql` arguments.
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
