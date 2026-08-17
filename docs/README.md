# Documentation

This index routes readers to the implemented preview runtime, deployment status, and validation contracts.

| Document | Covers |
| --- | --- |
| [Architecture](architecture/README.md) | Service boundaries, components, data flows, and authentication. |
| [Grafana query tools](grafana-query/README.md) | Canonical contracts for ten read actions and bounded silence creation. |
| [Grafana render tool](grafana-render/README.md) | Canonical contracts for bounded dashboard and panel PNG rendering. |
| [Tekton tools](tekton/README.md) | Implemented contracts for repository, workflow, run, task, log, and mutation actions. |
| [Kubernetes tools](kubernetes/README.md) | Locally implemented contracts for bounded multi-cluster reads and curated exact-object mutations. |
| [Ceph Dashboard tools](ceph/README.md) | Locally implemented contracts for bounded native Ceph reads and five curated OSD mutations. |
| [Machine deploys](deploys/README.md) | Machine inventory, fixed deploy workflows, SSH host trust, and OpenBao user certificates. |
| [Operations](operations/README.md) | Implemented deployment declarations, delivery behavior, and external-action boundaries. |
| [Quality](quality/README.md) | Verified local commands, current evidence, and remaining validation. |

The repository implements hosted OAuth, authenticated `/mcp`, integration tools, machine inventory, and fixed deploy execution. Deploy and OpenBao behavior has local and declaration evidence only; no live machine deploy occurred.

The current global `/mcp` scope set is `mcp:use kubernetes:read kubernetes:write inventory:read inventory:write inventory:host-trust deploy:read deploy:run`. Kuri requires the entire set for every tool and does not enforce scopes per tool or action. This wiring is locally tested but not deployed or verified through a browser flow.

The worktree implements the Ceph Dashboard runtime, MCP tools and tests, and preview-only Pulumi declarations. It adds no per-tool OAuth enforcement: every principal authorized for `/mcp` receives both Ceph query and exec authority. Local Rust and Pulumi checks pass. An authorized targeted preview apply seeded four Stashes from shared Dashboard administrator credentials without updating the application Secret or Deployment. No Dashboard account creation, authenticated Ceph call, live mutation, or preview rollout has occurred.

Preview runs commit `798dd92`. Basic health, readiness, OAuth metadata, and Bearer-challenge behavior are verified. An authenticated `alert-rule.list` call returned at least 100 entries; `recording-rule.list` returned `invalid_response` with `limit: 1`. The approved normalization fixes, Tekton, and Kubernetes changes remain undeployed. Full browser OAuth, other authenticated MCP and Grafana behavior, rendering, silence creation, effective RBAC, and expanded permission operation remain unverified. Production remains excluded and has zero resources.
