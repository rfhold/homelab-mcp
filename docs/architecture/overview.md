# Architecture Overview

## Status

The repository implements hosted OAuth, generic OIDC, PostgreSQL persistence, authenticated MCP, ten bounded Grafana query reads, two bounded Grafana image renders, bounded silence creation, and bounded Tekton and PAC tools in the current worktree.

Preview runs the authenticated runtime from commit `4f2e192`. Health, readiness, OAuth metadata, and the unauthenticated MCP Bearer challenge are verified. The worktree dashboard inventory, rendering, alerting, recording-rule, and Tekton revisions have not been deployed or operated live. Full browser OAuth, authenticated preview MCP calls, live integration behavior, renderer operation, and permission operation remain unverified; production remains excluded.

## Purpose

`homelab-mcp` exposes bounded homelab integrations through MCP. Grafana tools provide normalized reads, validated PNG rendering, and silence creation. Tekton tools add PAC-authorized repository, workflow, run, task, log, and mutation access in the current worktree.

The worktree adds typed multi-cluster reads and curated exact-object mutations through fixed `kubectl` command construction. Local implementation and tests exist; deployment, live-cluster, and effective-RBAC evidence remain pending.

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
- uses `#[mcp::progressive_server]` to generate read-only query and operationally consequential exec tools for Grafana, Tekton, and Kubernetes;
- exposes ten Grafana query actions, two Grafana render actions, bounded silence creation, eight Tekton reads, three Tekton mutations, four Kubernetes reads, and five Kubernetes mutations;
- queries Grafana's HTTP API through fixed Loki, Mimir, Tempo, and Pyroscope datasource UIDs;
- reads Grafana dashboard inventory, renders dashboard and panel PNGs, and partitions alert and recording rules from one fixed provisioning route;
- enforces local OAuth access tokens before MCP request handling; and
- persists generic OAuth and OIDC state in PostgreSQL schema `mcp`.

The [Grafana tool specifications](../grafana-query/README.md) and [Tekton tool specifications](../tekton/README.md) own implemented tool behavior. The [access document](access-authentication.md) owns authentication and authorization details.

The [Kubernetes tool specifications](../kubernetes/README.md) own implemented Kubernetes behavior, bounds, resource scope, and mutation safety.

## Component Status

| Component | Status | Responsibility |
| --- | --- | --- |
| Axum host | Deployed to preview | Initialize dependencies, serve health/runtime routes, and handle graceful shutdown. |
| Container declaration | Applied to preview | Build release and runtime images; Rust tests run through Cargo outside the image build. |
| Deployment declarations | Previous revision applied; worktree update pending | Supply the runtime variables, Secrets, mounts, identity, database, Grafana, workload, and route. |
| MCP endpoint | Worktree updated; previous revision deployed | Negotiate stateless Streamable HTTP and dispatch authenticated tool calls; the deployed unauthenticated challenge is verified. |
| Hosted OAuth issuer | Deployed to preview | Publish metadata, issue local access tokens, and manage durable OAuth client and token state; metadata is verified. |
| Generic OIDC integration | Deployed, flow unverified | Use MCP-owned one-shot OIDC transactions and hosted continuation with Authentik. |
| Grafana integration | Worktree implemented; deployment pending | Own fixed-destination datasource, dashboard, rendering, and alerting requests, validation, normalization, safe errors, and bounded telemetry. Rendering has local/mock evidence only. |
| Tekton integration | Worktree implemented; deployment pending | Own PAC repository authority, fixed Forgejo and PAC access, Kubernetes run and task access, normalized results, and bounded mutations. |
| Kubernetes integration | Worktree implemented; deployment pending | Own the configured cluster catalog, typed reads, normalized results, fixed mutations, process bounds, and safe errors. |
| PostgreSQL use | Deployed to preview | Store generic OAuth state and encrypted signing material through migrations V1-V3, with one-shot OIDC attempts added by V4. |
| Wrapping-key use | Deployed to preview | Load the mounted keyring and protect persisted OAuth signing keys. |

## MCP Request Flow

1. An MCP client discovers the service's hosted OAuth metadata.
2. The service redirects browser authentication to Authentik.
3. The service completes browser authentication and issues its own access token.
4. The client sends the local access token to `/mcp`.
5. The service validates the token and required `mcp:use kubernetes:read kubernetes:write` scope set in the current worktree.
6. The service validates the selected query or exec action arguments.
7. The service contacts only the action's fixed Grafana, Forgejo, PAC, or Kubernetes route with the integration-specific credential.
8. The action returns a semantic `McpToolResult`.

## Trust Boundaries

- Authentik authenticates a browser user. It does not issue tokens accepted by `/mcp`.
- The local OAuth issuer authorizes MCP access.
- The current worktree requires global scopes `mcp:use kubernetes:read kubernetes:write` for all `/mcp` actions, including Grafana and Tekton actions. The scopes do not enforce permissions per action.
- Dedicated reduced Kubernetes ServiceAccounts enforce upstream authority independently from OAuth.
- Grafana receives only server-originated requests with the service-account token.
- Tekton access uses fixed Forgejo, PAC controller, and Kubernetes destinations. Callers do not control credentials or upstream routes.
- Loki remains behind Grafana and has no direct service integration.
- PostgreSQL and the wrapping-key file hold separate parts of durable OAuth protection.

## Excluded Systems

Kuri-specific native APIs, agent sessions, S3 storage, and direct Loki access are outside this service boundary.
