# Architecture

These documents define the system boundary and access model. The implementation passes local tests against a reviewed immutable Kuri Git revision.

Preview runs commit `798dd92`. Basic runtime and discovery boundaries are verified. Authenticated rule-list evidence covers one successful alert-rule call and one failed recording-rule call. The approved Grafana normalization fixes, Tekton, and Kubernetes changes remain undeployed. Full browser OAuth, other authenticated MCP and Grafana behavior, effective Kubernetes RBAC, rendering, silence creation, and permission operation remain unverified. Production remains excluded.

| Document | Covers |
| --- | --- |
| [Overview](overview.md) | Components, dependencies, trust boundaries, and request flow. |
| [Access and authentication](access-authentication.md) | Hosted MCP OAuth roles, token boundaries, scopes, and durable state. |
| [Observability](observability.md) | Signal paths, bounded telemetry attributes, lifecycle, and data safety. |
| [Tekton tools](../tekton/README.md) | Planned integration behavior and evidence boundaries. |
| [Kubernetes tools](../kubernetes/README.md) | Locally implemented multi-cluster tool, authority, and safety contracts. |
