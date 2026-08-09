# Architecture Overview

## Status

The repository implements a health-only host and deployment foundation. MCP, OAuth runtime, database use, and Grafana queries remain planned.

## Purpose

`homelab-mcp` will expose bounded Grafana LogQL access through MCP. It will not expose Grafana credentials or direct Loki access to clients.

## Service Boundary

### Implemented Host

`src/main.rs` implements a Rust 1.96, edition 2024 Axum process. It binds `0.0.0.0:14333` and shuts down on SIGTERM or Ctrl-C.

`GET /health` and `GET /ready` return unconditional HTTP 200 responses. They do not check dependencies because the process uses no external dependency yet.

The process has no `/mcp` route, OAuth route, browser callback, database connection, or Grafana client.

### Planned MCP Service

The planned service will:

- use Rust and Kuri's generic private `mcp` crate at commit `302fd702ffdcf89ab4829f3a299486fc297406f9`;
- serve MCP through Streamable HTTP revision `2026-07-28` at `/mcp`;
- expose one progressive tool, `grafana_exec`;
- expose one initial action, `logql`;
- query Grafana's HTTP API through the fixed datasource UID `loki`;
- enforce local OAuth access tokens before MCP request handling; and
- persist OAuth and browser-auth state in PostgreSQL.

The [LogQL specification](../grafana-exec/spec/logql.md) owns request and response limits. The [access document](access-authentication.md) owns authentication and authorization details.

## Component Status

| Component | Status | Responsibility |
| --- | --- | --- |
| Axum host | Implemented | Serve unconditional health endpoints and handle graceful shutdown. |
| Container image | Implemented | Package the host as a non-root Debian bookworm runtime. |
| Deployment declarations | Implemented, not applied | Define future cluster, identity, Grafana, secret, workload, and route resources. |
| MCP endpoint | Planned | Negotiate Streamable HTTP and dispatch authenticated tool calls. |
| Hosted OAuth issuer | Planned | Issue local access tokens and manage OAuth client state. |
| Browser-auth integration | Planned at runtime | Use declared Authentik credentials to authenticate the browser user. |
| Grafana adapter | Planned | Send bounded instant or range queries through Grafana. |
| PostgreSQL use | Planned | Store generic OAuth state and browser-auth state. |
| Wrapping-key use | Planned | Protect persisted OAuth signing keys with the declared key file. |

## Planned MCP Request Flow

1. An MCP client discovers the service's hosted OAuth metadata.
2. The service redirects browser authentication to Authentik.
3. The service completes browser authentication and issues its own access token.
4. The client sends the local access token to `/mcp`.
5. The service validates the token and required `mcp:use` scope.
6. The service validates the `grafana_exec` and `logql` arguments.
7. The service queries Grafana with its Viewer service-account token.
8. The service returns a capped MCP tool result.

## Trust Boundaries

- Authentik authenticates a browser user. It does not issue tokens accepted by `/mcp`.
- The local OAuth issuer authorizes MCP access.
- Grafana receives only server-originated requests with the service-account token.
- Loki remains behind Grafana and has no direct service integration.
- PostgreSQL and the wrapping-key file hold separate parts of durable OAuth protection.

## Excluded Systems

Kuri-specific native APIs, agent sessions, S3 storage, dashboards, and direct Loki access are outside this service boundary.
