# Deploy Execution Specification

## Purpose

This specification governs the progressive `deploys` MCP tool, catalog resolution, and runtime execution.

## Requirements

### Requirement: Catalog-only execution

The service MUST resolve deploy IDs through `deploys/catalog.json`. It MUST reject unknown IDs and entries without `mcp_available`.

`bootstrap-homelab` MUST remain unavailable through MCP. `system-info` is the only current MCP deploy.

### Requirement: Exact target

`run` MUST accept one deploy ID and one exact machine UUID. It MUST NOT accept arbitrary commands, arguments, environment variables, paths, SSH options, or multiple targets.

### Requirement: Host trust before credentials

The runner MUST reject a machine without an exact host pin before key generation, OpenBao access, or pyinfra execution.

### Requirement: Ephemeral certificate identity

Each run MUST generate a fresh Ed25519 client key. OpenBao MUST sign its public key for principal `homelab` with TTL `15m`.

### Requirement: Fixed process

The runner MUST clear the child environment and invoke the configured absolute uv executable. Arguments MUST match `run --locked pyinfra --yes <inventory> <entrypoint>`.

### Requirement: Strict host checking

The runner MUST build one known-hosts entry from inventory. pyinfra MUST enable strict host key checking.

### Requirement: Resource bounds

The service MUST allow one active deploy. It MUST enforce catalog timeout, stdout, stderr, and temporary credential bounds.

### Requirement: Unknown outcomes

After process launch, cancellation, timeout, output overflow, or execution failure can produce an unknown outcome. The service MUST NOT retry automatically.

## References

- [`deploys/catalog.json`](../../../deploys/catalog.json)
- [`src/integrations/deploys/catalog.rs`](../../../src/integrations/deploys/catalog.rs), `DeployCatalog`
- [`src/integrations/deploys/runner.rs`](../../../src/integrations/deploys/runner.rs), `DeployRunner`
- [`src/integrations/deploys/openbao.rs`](../../../src/integrations/deploys/openbao.rs), `OpenBaoClient`
- [Deploy workflow](../deploy-workflow.md)
- [SSH trust and OpenBao identity](../ssh-trust-openbao.md)
