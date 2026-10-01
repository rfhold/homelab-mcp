# Operations

These documents describe deployment declarations and pipeline behavior. Preview runs the authenticated OAuth/MCP runtime.

The runtime uses the reviewed immutable Kuri Git revision, and the container and pipeline declarations can resolve it. A signed stable-tag pipeline declares digest-only production promotion from existing preview artifacts. Production remains unapplied with zero resources; release execution and credentials require separate authority.

| Document | Covers |
| --- | --- |
| [Deployment](deployment.md) | Pulumi stacks, Kubernetes resources, credentials, preview builds, signed stable-tag promotion, and operational constraints. |
| [Observability](observability.md) | Telemetry configuration, validation, lifecycle, and troubleshooting. |
| [Ceph Dashboard deployment and access](../ceph/spec/deployment-access.md) | Declared preview credentials, destinations, networking, production exclusion, and independent approval gates. |
| [Machine deploy operations](../deploys/README.md) | Bootstrap, host trust reset, OpenBao identity, fixed deploy execution, and validation boundaries. |
