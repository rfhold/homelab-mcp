# Documentation

This index routes readers to the implemented working-tree runtime, deployed foundation, and validation contracts.

| Document | Covers |
| --- | --- |
| [Architecture](architecture/README.md) | Service boundaries, components, data flows, and authentication. |
| [Grafana Query](grafana-query/README.md) | The implemented progressive read-only tool and its four action contracts. |
| [Operations](operations/README.md) | Implemented deployment declarations, delivery behavior, and external-action boundaries. |
| [Quality](quality/README.md) | Verified local commands, current evidence, and remaining validation. |

The repository implements hosted OAuth, authenticated `/mcp`, and progressive `grafana_query` actions for LogQL, PromQL, TraceQL, and Profiles.

Locked Cargo, Docker, and Tekton builds can resolve the reviewed dependency. Preview still runs the prior health-only image. Production remains excluded and has zero resources.
