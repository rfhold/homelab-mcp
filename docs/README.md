# Documentation

This index routes readers to the implemented preview runtime, deployment status, and validation contracts.

| Document | Covers |
| --- | --- |
| [Architecture](architecture/README.md) | Service boundaries, components, data flows, and authentication. |
| [Grafana query tools](grafana-query/README.md) | Canonical contracts for nine read actions and bounded silence creation. |
| [Grafana render tool](grafana-render/README.md) | Canonical contracts for bounded dashboard and panel PNG rendering. |
| [Tekton tools](tekton/README.md) | Implemented contracts for repository, workflow, run, task, log, and mutation actions. |
| [Operations](operations/README.md) | Implemented deployment declarations, delivery behavior, and external-action boundaries. |
| [Quality](quality/README.md) | Verified local commands, current evidence, and remaining validation. |

The repository implements hosted OAuth, authenticated `/mcp`, nine read-only `grafana_query` actions, two `grafana_render` actions, `grafana_exec` action `silence.create`, and separate `tekton_query` and `tekton_exec` tools. The existing `mcp:use` scope authorizes every action.

Preview runs the previously deployed authenticated runtime. Basic health, readiness, OAuth metadata, and Bearer-challenge behavior are verified. The dashboard inventory, rendering, alerting, and Tekton worktree revisions have not been deployed or exercised live. Full browser OAuth, authenticated preview MCP calls, live integration behavior, renderer operation, and expanded permission operation remain unverified. Production remains excluded and has zero resources.
