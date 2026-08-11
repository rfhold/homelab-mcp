# Architecture

These documents define the system boundary and access model. The implementation passes local tests against a reviewed immutable Kuri Git revision.

The previously committed runtime is deployed to preview. Basic runtime and discovery boundaries are verified. The worktree Grafana, Tekton, and Kubernetes expansions are locally implemented but not deployed; full browser OAuth, authenticated preview MCP calls, live integration behavior, effective Kubernetes RBAC, renderer operation, and permission operation remain unverified. Production remains excluded.

| Document | Covers |
| --- | --- |
| [Overview](overview.md) | Components, dependencies, trust boundaries, and request flow. |
| [Access and authentication](access-authentication.md) | Hosted MCP OAuth roles, token boundaries, scopes, and durable state. |
| [Observability](observability.md) | Signal paths, bounded telemetry attributes, lifecycle, and data safety. |
| [Tekton tools](../tekton/README.md) | Planned integration behavior and evidence boundaries. |
| [Kubernetes tools](../kubernetes/README.md) | Locally implemented multi-cluster tool, authority, and safety contracts. |
