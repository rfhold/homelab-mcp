# Architecture Overview

## Status

The repository implements hosted OAuth, generic OIDC, PostgreSQL persistence, authenticated MCP, ten bounded Grafana query reads, two bounded Grafana image renders, bounded silence creation, and bounded Tekton and PAC tools in the current worktree.

Preview runs the authenticated runtime from commit `798dd92`. Health, readiness, OAuth metadata, and the unauthenticated MCP Bearer challenge are verified. An authenticated `alert-rule.list` call returned at least 100 entries; `recording-rule.list` returned `invalid_response` with `limit: 1`. The approved rule-normalization fixes and the worktree Tekton and Kubernetes changes remain undeployed. Full browser OAuth, other authenticated MCP and Grafana behavior, rendering, silence creation, effective Kubernetes RBAC, and permission operation remain unverified; production remains excluded.

## Purpose

`homelab-mcp` exposes bounded homelab integrations through MCP. Grafana tools provide normalized reads, validated PNG rendering, and silence creation. Tekton tools add PAC-authorized repository, workflow, run, task, log, and mutation access in the current worktree.

The worktree adds typed multi-cluster reads and curated exact-object mutations through fixed `kubectl` command construction. Local implementation and tests exist; deployment, live-cluster, and effective-RBAC evidence remain pending.

The worktree implements bounded native Ceph reads and five curated OSD mutations for Pantheon and Romulus. Rust tests cover the runtime and MCP registration. Preview-only Pulumi declarations and mock tests cover cluster configuration and credential projection. No Ceph deployment or live evidence exists.

The worktree also implements PostgreSQL machine inventory and a fixed uv/pyinfra deploy subsystem. It uses exact SSH host pins and ephemeral OpenBao user certificates. Local tests and preview-only declarations exist. No live deploy or OpenBao SSH flow has run.

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
- uses `#[mcp::progressive_server]` to generate read-only query and operationally consequential exec tools for Grafana, Tekton, Kubernetes, and Ceph;
- exposes ten Grafana query actions, two Grafana render actions, bounded silence creation, eight Tekton reads, three Tekton mutations, four Kubernetes reads, five Kubernetes mutations, ten Ceph reads, and five Ceph OSD mutations;
- exposes progressive `machines` inventory actions and `deploys` catalog and run actions;
- queries Grafana's HTTP API through fixed Loki, Mimir, Tempo, and Pyroscope datasource UIDs;
- reads Grafana dashboard inventory, renders dashboard and panel PNGs, and partitions alert and recording rules from one fixed provisioning route;
- enforces local OAuth access tokens before MCP request handling; and
- persists generic OAuth and OIDC state in PostgreSQL schema `mcp`.

The [Grafana tool specifications](../grafana-query/README.md), [Tekton tool specifications](../tekton/README.md), and [machine deploy specifications](../deploys/README.md) own implemented tool behavior. The [access document](access-authentication.md) owns authentication and authorization details.

The [Kubernetes tool specifications](../kubernetes/README.md) own implemented Kubernetes behavior, bounds, resource scope, and mutation safety. The [Ceph Dashboard specifications](../ceph/README.md) own implemented native Ceph behavior and its boundary with coarse Rook controller-state reads.

## Component Status

| Component | Status | Responsibility |
| --- | --- | --- |
| Axum host | Deployed to preview | Initialize dependencies, serve health/runtime routes, and handle graceful shutdown. |
| Container declaration | Applied to preview | Build release and runtime images; Rust tests run through Cargo outside the image build. |
| Deployment declarations | Commit `798dd92` applied; worktree update pending | Supply the runtime variables, Secrets, mounts, identity, database, Grafana, workload, and route. |
| MCP endpoint | Commit `798dd92` deployed; worktree update pending | Negotiate stateless Streamable HTTP and dispatch authenticated tool calls; the deployed unauthenticated challenge and partial authenticated rule-list behavior are verified. |
| Hosted OAuth issuer | Deployed to preview | Publish metadata, issue local access tokens, and manage durable OAuth client and token state; metadata is verified. |
| Generic OIDC integration | Deployed, flow unverified | Use MCP-owned one-shot OIDC transactions and hosted continuation with Authentik. |
| Grafana integration | Commit `798dd92` deployed; normalization fixes pending | Own fixed-destination datasource, dashboard, rendering, and alerting requests, validation, normalization, safe errors, and bounded telemetry. Live evidence covers only the two rule-list calls described above. |
| Tekton integration | Worktree implemented; deployment pending | Own PAC repository authority, fixed Forgejo and PAC access, Kubernetes run and task access, normalized results, and bounded mutations. |
| Kubernetes integration | Worktree implemented; deployment pending | Own the configured cluster catalog, typed reads, normalized results, fixed mutations, process bounds, and safe errors. |
| Ceph Dashboard integration | Worktree implemented and locally verified; deployment pending | Own fixed Pantheon and Romulus Dashboard destinations, native Ceph reads, five curated OSD mutations, reviewed task identities, and safe errors. |
| Machine deploy integration | Worktree implemented and locally verified; deployment pending | Own PostgreSQL inventory, fixed deploy catalog, OpenBao user certificates, strict host trust, and bounded pyinfra processes. |
| PostgreSQL use | Deployed to preview | Store generic OAuth state and encrypted signing material through migrations V1-V3, with one-shot OIDC attempts added by V4. |
| Wrapping-key use | Deployed to preview | Load the mounted keyring and protect persisted OAuth signing keys. |

## MCP Request Flow

1. An MCP client discovers the service's hosted OAuth metadata.
2. The service redirects browser authentication to Authentik.
3. The service completes browser authentication and issues its own access token.
4. The client sends the local access token to `/mcp`.
5. The service validates the token and every scope in `Config::REQUIRED_OAUTH_SCOPES`.
6. The service validates the selected query or exec action arguments.
7. The service contacts only fixed integration routes, or runs one catalog deploy against one exact machine.
8. The action returns a semantic `McpToolResult`.

## Trust Boundaries

- Authentik authenticates a browser user. It does not issue tokens accepted by `/mcp`.
- The local OAuth issuer authorizes MCP access.
- Kuri requires `mcp:use kubernetes:read kubernetes:write inventory:read inventory:write inventory:host-trust deploy:read deploy:run` for every `/mcp` action. It does not enforce scopes per tool or action.
- Machine SSH host identity comes only from an exact stored Ed25519 pin. OpenBao supplies short-lived user certificates, not host trust.
- Dedicated reduced Kubernetes ServiceAccounts enforce upstream authority independently from OAuth.
- Preview uses each cluster's Rook-generated shared Dashboard administrator credential under an explicit exception. Dedicated least-privilege accounts remain a production gate. OAuth grants every authenticated MCP principal both Ceph query and exec authority.
- Grafana receives only server-originated requests with the service-account token.
- Tekton access uses fixed Forgejo, PAC controller, and Kubernetes destinations. Callers do not control credentials or upstream routes.
- Loki remains behind Grafana and has no direct service integration.
- PostgreSQL and the wrapping-key file hold separate parts of durable OAuth protection.

## Excluded Systems

Kuri-specific native APIs, agent sessions, S3 storage, and direct Loki access are outside this service boundary.
