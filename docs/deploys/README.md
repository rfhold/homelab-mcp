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

`deploys/catalog.json` defines two approved deploys. `bootstrap-homelab` is operator-local, high-risk, mutating, agent-first with method-gated password fallback, and unavailable through MCP. `system-info` is low-risk, read-only, certificate-based, and available through MCP. The local `ssh_homelab` helper provides short-lived OpenBao-certified human sessions after bootstrap and reuses only its least-privilege OIDC SSH-signing token from a protected volatile runtime cache.

No live machine deploy, bootstrap, OpenBao call, SSH connection, or preview apply occurred for this change. The platform-side eight-hour OIDC role/policy work and this repository's explicit 30-second Pulumi role declaration must be verified independently after deployment.
