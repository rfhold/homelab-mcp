# Ceph Dashboard Tools Shared Contract

## Status

This document defines locally implemented behavior. Rust runtime code, MCP registration and tests, and preview-only Pulumi declarations provide local evidence. An authorized targeted preview apply seeded four Pulumi Stashes from the Rook-generated shared Dashboard administrator credentials; it did not update the application Secret or Deployment. No Dashboard account creation, authenticated Ceph read, live Ceph mutation, or preview rollout has occurred. Production configuration explicitly disables Ceph through an empty catalog; no production Ceph resource or apply has occurred.

## Tool Surfaces

The authenticated MCP server exposes two typed progressive tools:

| Tool | Actions | MCP annotations |
| --- | --- | --- |
| `ceph_query` | `cluster.list`, `status.get`, `metrics.summary`, `osd.list`, `osd.get`, `osd.safe-to-destroy`, `device.list`, `device.get`, `flags.get`, `task.list` | `readOnlyHint: true`, `destructiveHint: false`, `idempotentHint: true`, `openWorldHint: true` |
| `ceph_exec` | `osd.mark`, `osd.reweight`, `osd.scrub`, `osd.destroy`, `osd.purge` | `readOnlyHint: false`, `destructiveHint: true`, `idempotentHint: false`, `openWorldHint: true` |

Each tool provides progressive `help` actions, action-dependent typed `input`, and an optional jq-compatible `filter`. Read actions exist only on `ceph_query`; mutations exist only on `ceph_exec`.

The service constructs requests to fixed Ceph Dashboard API operations. A caller cannot provide an origin, route, path, query string, HTTP method, headers, credential, request body, Ceph command, or CLI argument.

## Authorization Risk

The [global OAuth contract](../../architecture/access-authentication.md#protocol-boundary) protects the complete `/mcp` resource. It has no per-tool or per-action enforcement. No Ceph-specific OAuth scope separates reads from mutations.

Every principal that passes the global `/mcp` authorization boundary can call both `ceph_query` and `ceph_exec`. This grants every authenticated MCP principal authority to mark, reweight, scrub, destroy, and purge OSDs in either configured cluster. Preview uses each cluster's Rook-generated shared Dashboard administrator account, so the upstream credential has broader authority than the fixed MCP action catalog. This user-approved exception and the global authorization model are production blockers. Dedicated least-privilege Dashboard accounts remain required before production approval.

Every exec call requires an explicit user decision. Authentication alone does not authorize an operator or automated validation process to perform a particular live mutation.

## Cluster Authority

Preview configuration defines exactly two cluster selectors, `pantheon` and `romulus`. Each selector binds one fixed Ceph 19 Squid Dashboard HTTPS origin to one dedicated per-cluster Dashboard credential. Cluster names are case-sensitive stable MCP identities.

The catalog, not caller input, owns Dashboard origins, TLS destinations, credentials, and cluster membership. `cluster.list` exposes only safe cluster identities. It does not expose origins or credentials.

## Shared Request and Result Boundary

All upstream work uses bounded concurrency, a complete-operation deadline, bounded response reads, and strict action-specific input and result limits. Capacity exhaustion fails before dispatch. No action retries automatically.

OSD, device, and task list limits default to 50 and accept values from 1 through 100. Device identities are nonblank, control-free strings of at most 512 bytes. Status output contains at most 100 health checks, and each normalized free-text field contains at most 4,096 bytes. Focused specifications own narrower shape limits.

Successful results use typed normalized allowlists with explicit truncation metadata where a list can exceed its result bound. Results, errors, logs, traces, and metrics exclude credentials, authorization headers, Dashboard origins, internal routes, arbitrary upstream bodies, raw Ceph command output, and unapproved Dashboard response fields.

Read failures use bounded semantic codes and safe messages. Mutations use the outcome rules in the [mutation specification](mutations.md#outcome-semantics). A jq-compatible filter can reduce a successful normalized result, but it cannot expand the upstream request or expose omitted fields.

## Ownership Boundaries

Ceph Dashboard owns native Ceph operational state, including OSD state, safe-to-destroy evaluation, devices, cluster flags, current Dashboard metrics, and Dashboard task state.

The Kubernetes tools retain coarse reads of Rook `CephCluster`, `CephFilesystem`, `CephBlockPool`, and `CephObjectStore` custom resources. Those reads describe Kubernetes controller state only. They do not replace native Ceph status or authorize native Ceph operations. See the [Kubernetes resource contract](../../kubernetes/spec/queries-resources.md#rook-and-native-ceph-state).

## Exclusions

The tools do not provide:

- arbitrary Ceph commands, Dashboard paths, methods, headers, or request bodies;
- OSD `lost` or `up` operations;
- force options or automatic retries;
- historical Prometheus metrics or arbitrary metrics queries;
- crash archive operations, Rook daemon restarts, or physical device replacement;
- cluster flag mutation, pool mutation, repair actions, or general Ceph CLI networking;
- Ceph monitor exposure; or
- any query or mutation outside the fixed action catalogs.

Global cluster flag mutation is deferred because Squid Dashboard exposes only a race-prone full-list replacement operation. Crash archive, Rook daemon restart, physical replacement, pool mutation, repair, historical metrics, and general CLI networking are also deferred.
