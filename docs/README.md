# Documentation

This index routes readers to the implemented preview runtime, deployment status, and validation contracts.

| Document | Covers |
| --- | --- |
| [Architecture](architecture/README.md) | Service boundaries, components, data flows, and authentication. |
| [Grafana tools](grafana-query/README.md) | Canonical contracts for seven read actions and bounded silence creation. |
| [Tekton tools](tekton/README.md) | Implemented contracts for repository, workflow, run, task, log, and mutation actions. |
| [Operations](operations/README.md) | Implemented deployment declarations, delivery behavior, and external-action boundaries. |
| [Quality](quality/README.md) | Verified local commands, current evidence, and remaining validation. |

The repository implements hosted OAuth, authenticated `/mcp`, seven read-only `grafana_query` actions, `grafana_exec` action `silence.create`, and separate `tekton_query` and `tekton_exec` tools. The existing `mcp:use` scope authorizes every action.

Preview runs the previously deployed authenticated runtime. Basic health, readiness, OAuth metadata, and Bearer-challenge behavior are verified. The alerting and Tekton worktree revisions have not been deployed or exercised live. Full browser OAuth, authenticated preview MCP calls, live integration behavior, and expanded permission operation remain unverified. Production remains excluded and has zero resources.
