# Documentation

This index routes readers to the implemented preview runtime, deployment status, and validation contracts.

| Document | Covers |
| --- | --- |
| [Architecture](architecture/README.md) | Service boundaries, components, data flows, and authentication. |
| [Grafana query tools](grafana-query/README.md) | Canonical contracts for ten read actions and bounded silence creation. |
| [Grafana render tool](grafana-render/README.md) | Canonical contracts for bounded dashboard and panel PNG rendering. |
| [Tekton tools](tekton/README.md) | Implemented contracts for repository, workflow, run, task, log, and mutation actions. |
| [Kubernetes tools](kubernetes/README.md) | Locally implemented contracts for bounded multi-cluster reads and curated exact-object mutations. |
| [Operations](operations/README.md) | Implemented deployment declarations, delivery behavior, and external-action boundaries. |
| [Quality](quality/README.md) | Verified local commands, current evidence, and remaining validation. |

The repository implements hosted OAuth, authenticated `/mcp`, Grafana tools, Tekton tools, and the Kubernetes `kubernetes_query` and `kubernetes_exec` tools. Kubernetes runtime, OAuth wiring, Pulumi declarations, container support, and local tests exist; deployment, browser, live-cluster, and effective-RBAC evidence remain pending.

The Kubernetes implementation changes the global `/mcp` scope set to `mcp:use kubernetes:read kubernetes:write`. This wiring is locally tested but not deployed or verified through a browser flow.

Preview runs the previously deployed authenticated runtime. Basic health, readiness, OAuth metadata, and Bearer-challenge behavior are verified. The dashboard inventory, rendering, alerting, recording-rule, Tekton, and Kubernetes worktree revisions have not been deployed or exercised live. Full browser OAuth, authenticated preview MCP calls, live integration behavior, renderer operation, effective RBAC, and expanded permission operation remain unverified. Production remains excluded and has zero resources.
