# Kubernetes Tools Shared Contract

## Status

This document defines locally implemented behavior. Runtime code, deployment declarations, and local tests exist. Deployment, browser, live-cluster, and effective-RBAC evidence remain pending. Production remains unapplied.

## Tool Surfaces

The authenticated MCP server exposes two typed progressive tools:

| Tool | Actions | MCP annotations |
| --- | --- | --- |
| `kubernetes_query` | `cluster_list`, `capability_list`, `resource_list`, `resource_get` | `readOnlyHint: true`, `destructiveHint: false`, `idempotentHint: true`, `openWorldHint: true` |
| `kubernetes_exec` | `workload_restart`, `workload_scale`, `cronjob_suspend`, `cronjob_trigger`, `pod_delete` | `readOnlyHint: false`, `destructiveHint: true`, `idempotentHint: false`, `openWorldHint: true` |

Each tool provides progressive `help` actions, action-dependent typed `input`, and an optional jq-compatible `filter`. Read actions exist only on `kubernetes_query`. Mutation actions exist only on `kubernetes_exec`.

The implementation uses fixed `kubectl` command construction behind these typed actions. No tool exposes arbitrary `kubectl`, arguments, verbs, resources, API paths, selectors, or output templates.

## Authorization

The `/mcp` protected resource requires the exact global OAuth set: `mcp:use kubernetes:read kubernetes:write inventory:read inventory:write inventory:host-trust deploy:read deploy:run`. Kuri requests all eight scopes automatically during authorization.

The entire set gates the complete `/mcp` resource. It does not enforce permissions per tool or action. Every authorized MCP principal can use both Kubernetes tools and all other MCP tools.

Historical preview grants lack the seven expanded scopes. Each client and user from that revision must complete browser authorization again before the client can call the updated `/mcp`.

The [access contract](../../architecture/access-authentication.md) owns token validation, consent, challenge, and reauthorization behavior. Kubernetes ServiceAccount RBAC remains an independent upstream enforcement boundary.

## Cluster Authority

A server configuration catalog will define from one through 32 clusters. Catalog entries will use unique stable names and fixed kubeconfig contexts. The initial catalog will represent Pantheon and Romulus.

The catalog, not caller input, will define kubeconfig files, contexts, API servers, credentials, and cluster membership. A caller can select only an exact configured cluster name.

`src/integrations/kubernetes/catalog.rs` with `KubernetesCatalog` will own catalog dispatch. `src/integrations/kubernetes/client.rs` with `KubernetesConfig` and `KubernetesClient` will own one cluster connection.

## Shared Limits

| Boundary | Limit |
| --- | --- |
| Configured clusters | 1 through 32 |
| Concurrent `kubectl` processes per cluster client | 2 |
| Outer process deadline | At most 30 seconds |
| Captured stdout | 4 MiB |
| Captured stderr | 32 KiB |
| List pagination | At most 5 pages |
| Source objects inspected per list | At most 500 |
| Objects returned per list | At most 100 |
| Exact label-selector entries | At most 8 |
| Conditions per object | At most 20 |
| Event message | At most 1,024 UTF-8 bytes per event |
| Event messages per result | At most 32 KiB total |
| Scale replicas | 0 through 1,000 |

The service will enforce bounds before process launch and while it drains output. Capacity exhaustion will fail immediately and will not queue unbounded work.

## Results and Errors

Results will use typed normalized allowlists. They will exclude raw Kubernetes objects, managed fields, arbitrary labels, arbitrary annotations, credentials, internal paths, and raw stderr.

Lists will report inspected, returned, source-truncation, result-truncation, and aggregate-truncation metadata. Stable ordering and exact object identity will make bounded results deterministic.

Errors will use bounded semantic codes and safe messages. They will not expose command lines, kubeconfig content, bearer tokens, API origins, raw objects, stdout, or stderr.

No action will retry a `kubectl` process automatically. The [mutation contract](mutations.md#outcome-semantics) defines post-dispatch ambiguity.

## Exclusions

The tools will not provide:

- pod logs, because callers must use Grafana and Loki;
- raw objects, Secrets, ConfigMaps, or arbitrary custom resources;
- arbitrary arguments, verbs, resources, API paths, selectors, or output templates;
- arbitrary output labels or annotations;
- `exec`, `attach`, `debug`, `cp`, `proxy`, or `port-forward`;
- apply, general patch, general delete, or force delete;
- node drain, cordon, or taint operations; or
- any action outside the fixed query and mutation catalogs.

## Evidence Boundary

The contract requires local tests for schemas, command construction, normalization, limits, errors, and cancellation. Deployment tests must verify catalog and RBAC declarations.

Preview evidence must verify dedicated credentials, effective RBAC, read behavior, dry-run behavior, accepted mutations, and uncertain-outcome recovery. Each live read or mutation requires exact target-specific authority.

Production remains unapplied and outside current verification.
