# Documentation

This index routes readers to the implemented working-tree runtime, deployed foundation, and validation contracts.

| Document | Covers |
| --- | --- |
| [Architecture](architecture/README.md) | Service boundaries, components, data flows, and authentication. |
| [Grafana Exec](grafana-exec/README.md) | The implemented progressive tool and its LogQL contract. |
| [Operations](operations/README.md) | Implemented deployment declarations, delivery behavior, and external-action boundaries. |
| [Quality](quality/README.md) | Verified local commands, current evidence, and remaining validation. |

The repository implements hosted OAuth, authenticated `/mcp`, and progressive `grafana_exec`/`logql`. The 25-test Rust suite passes against the exact reviewed Kuri Git pin.

Locked Cargo, Docker, and Tekton builds can resolve the reviewed dependency. Preview still runs the prior health-only image. Production remains excluded and has zero resources.
