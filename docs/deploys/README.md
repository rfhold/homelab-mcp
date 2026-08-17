# Machine Deploys

These documents define the machine inventory, approved deploy catalog, SSH trust, and safe operator procedures.

| Document | Covers |
| --- | --- |
| [Machine inventory](machine-inventory.md) | PostgreSQL records, lifecycle, host pin reset, and data boundaries. |
| [Deploy workflow](deploy-workflow.md) | The uv and pyinfra project, catalog, bootstrap, MCP execution, and recovery. |
| [SSH trust and OpenBao identity](ssh-trust-openbao.md) | User certificates, workload identity, host authentication, credential lifecycle, and deployment declarations. |
| [Machine inventory specification](spec/machines.md) | Normative `machines` actions, validation, persistence, and host trust behavior. |
| [Deploy execution specification](spec/deploys.md) | Normative `deploys` actions, fixed invocation, bounds, and failure behavior. |
| [Service deployment](../operations/deployment.md#machine-deploy-access) | Preview declarations, CI credential boundaries, and apply gates. |
| [Testing](../quality/testing.md#machine-deploy-contract-coverage) | Local evidence and unverified runtime layers. |

`deploys/catalog.json` defines two approved deploys. `bootstrap-homelab` is operator-local, high-risk, mutating, and unavailable through MCP. `system-info` is low-risk, read-only, and available through MCP.

No live machine deploy, bootstrap, OpenBao login, SSH certificate authentication, or preview apply occurred for this documentation change.
