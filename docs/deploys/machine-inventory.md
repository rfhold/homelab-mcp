# Machine Inventory

## Authority

The migration `migrations/20260813000000_create_machines.sql` owns the PostgreSQL shape. `src/inventory.rs` owns validation and repository behavior. [The machine specification](spec/machines.md) owns the MCP contract.

The application starts one shared `PgPool`. `src/main.rs` calls `database::migrate` before OAuth initialization, service construction, or HTTP service. `MachineRepository` receives that shared pool.

## Data Model

PostgreSQL table `homelab.machines` stores one exact SSH target per UUID:

| Field | Purpose |
| --- | --- |
| `id` | Application-generated UUID and primary key. |
| `display_name` | Unique operator label. |
| `ssh_host` | DNS name, IPv4 address, or IPv6 address. |
| `ssh_port` | Fixed port 22, matching the approved preview NetworkPolicy boundary; new records default to 22. |
| `ssh_username` | Fixed remote account `homelab`; new records default to `homelab`. |
| `pinned_host_public_key` | Optional exact Ed25519 host public key. |
| `created_at`, `updated_at` | Server timestamps. |

The inventory has no group model. It stores no passwords, private keys, OpenBao tokens, SSH certificates, arbitrary SSH options, or deploy arguments.

## Lifecycle

The progressive `machines` tool exposes `list`, `create`, `update`, `delete`, `host-key.clear`, and `host-key.replace`. Updates change connection fields and preserve host trust. Host trust changes use dedicated actions.

Use this reset sequence after a legitimate host key change:

1. Call `host-key.clear` for the exact machine UUID.
2. Treat all deploys to that machine as blocked.
3. Verify the replacement Ed25519 host public key through an independent, trusted channel.
4. Call `host-key.replace` with the exact `ssh-ed25519 <base64>` public key.
5. Run a read-only approved deploy before any future mutating workflow.

Do not use `ssh-keyscan`, first-connection acceptance, or trust on first use. A cleared pin blocks deploy execution before key generation, OpenBao access, or SSH execution.

Delete removes the inventory record only. It does not change the machine, revoke certificates, remove OpenBao trust, or undo bootstrap changes.
