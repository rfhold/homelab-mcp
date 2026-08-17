# Machine Inventory Specification

## Purpose

This specification governs the progressive `machines` MCP tool and `homelab.machines` persistence.

## Requirements

### Requirement: Exact records

The service MUST address mutations by one valid machine UUID. It MUST NOT implement machine groups or group mutation.

### Requirement: Bounded fields

The service MUST validate display names, SSH hosts, ports, usernames, list limits, and exact Ed25519 host public keys. A host key MUST use `ssh-ed25519 <base64>` without options or comments.

### Requirement: Separate host trust

`update` MUST preserve `pinned_host_public_key`. Only `host-key.clear` and `host-key.replace` can change host trust.

#### Scenario: Clear blocks deploy

- Given a machine with a valid host pin
- When `host-key.clear` succeeds
- Then the record has no host pin
- And deploy execution rejects that machine before helper execution

### Requirement: No secrets

Inventory MUST NOT store SSH private keys, passwords, OpenBao credentials, or certificates.

### Requirement: Mutation uncertainty

If an MCP request cancels during an inventory mutation, the tool MUST return `mutation_outcome_unknown`. The caller MUST inspect inventory before a retry.

## References

- [`src/inventory.rs`](../../../src/inventory.rs), especially `MachineRepository` and `validate_host_public_key`
- [`src/mcp.rs`](../../../src/mcp.rs), progressive `machines` actions
- [`migrations/20260813000000_create_machines.sql`](../../../migrations/20260813000000_create_machines.sql)
- [Machine inventory](../machine-inventory.md)
