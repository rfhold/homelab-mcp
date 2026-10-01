# Kubernetes Tools Shared Contract

## Status

This document defines locally implemented behavior. Runtime code, deployment declarations, and local tests exist. Deployment, browser, live-cluster, and effective-RBAC evidence remain pending. Production remains unapplied.

## Tool Surfaces

Cluster and capability discovery are resources; live state and logs use `query`, restart/scale/suspend use `execute`, CronJob triggering uses `create`, and Pod deletion uses `destroy`. The [uniform MCP interface](../../architecture/mcp-interface.md) owns exact public domain-prefixed names, schemas, resources, annotations, and routing. Focused specifications retain unqualified backend operation labels. No help actions or compatibility aliases exist.

Typed tool inputs and jq projections retain their existing semantics, synchronized text, image content, and unchanged semantic tool errors. Resource failures use JSON-RPC errors with safe semantic details. The same global authorization, fixed upstream operations, cancellation, and bounds apply.

The implementation uses fixed `kubectl` command construction. No interface exposes arbitrary arguments, verbs, API paths, selectors, output templates, or manifests.

## Authorization

The `/mcp` protected resource requires the exact global OAuth set: `mcp:use kubernetes:read kubernetes:write inventory:read inventory:write inventory:host-trust deploy:read deploy:run`. Kuri requests all eight scopes automatically during authorization.

The entire set gates the complete `/mcp` resource. It does not enforce permissions per tool or action. Every authorized MCP principal can use Kubernetes resources and tools and all other MCP tools.

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
| Pod container lifecycle statuses | At most 32 total |
| Pod log tail | 1 through 1,000 lines; default 200 |
| Pod log output | 1 through 262,144 UTF-8 bytes; default 65,536 |
| Scale replicas | 0 through 1,000 |

The service will enforce bounds before process launch and while it drains output. Capacity exhaustion will fail immediately and will not queue unbounded work.

## Results and Errors

Results will use typed normalized allowlists. They will exclude raw Kubernetes objects, managed fields, arbitrary labels, arbitrary annotations, credentials, internal paths, and raw stderr.

Lists will report inspected, returned, source-truncation, result-truncation, and aggregate-truncation metadata. Stable ordering and exact object identity will make bounded results deterministic.

Errors will use bounded semantic codes and safe messages. They will not expose command lines, kubeconfig content, bearer tokens, API origins, raw objects, stdout, or stderr.

No action will retry a `kubectl` process automatically. The [mutation contract](mutations.md#outcome-semantics) defines post-dispatch ambiguity.

## Exclusions

The tools will not provide:

- raw objects, Secrets, ConfigMaps, or arbitrary custom resources;
- arbitrary arguments, verbs, resources, API paths, selectors, or output templates;
- arbitrary output labels or annotations;
- caller-selected log URLs or query parameters, log following, or log streaming;
- `exec`, `attach`, `debug`, `cp`, `proxy`, or `port-forward`;
- apply, general patch, general delete, or force delete;
- node drain, cordon, or taint operations; or
- any action outside the fixed query and mutation catalogs.

## Evidence Boundary

The contract requires local tests for schemas, command construction, normalization, limits, errors, and cancellation. Deployment tests must verify catalog and RBAC declarations.

Preview evidence must verify dedicated credentials, effective RBAC, read behavior, dry-run behavior, accepted mutations, and uncertain-outcome recovery. Each live read or mutation requires exact target-specific authority.

Production remains unapplied and outside current verification.
