# Operations

These documents describe deployment declarations and pipeline behavior. Preview runs the authenticated OAuth/MCP runtime.

The runtime uses the reviewed immutable Kuri Git revision, and the container and pipeline declarations can resolve it. Production remains excluded with zero resources.

| Document | Covers |
| --- | --- |
| [Deployment](deployment.md) | Pulumi stacks, Kubernetes resources, credentials, delivery inputs, and operational constraints. |
| [Observability](observability.md) | Telemetry configuration, validation, lifecycle, and troubleshooting. |
| [Ceph Dashboard deployment and access](../ceph/spec/deployment-access.md) | Declared preview credentials, destinations, networking, production exclusion, and independent approval gates. |
