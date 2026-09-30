# Machine And Deploy Actions

Read/discovery actions: `machines` action `list` takes optional `limit` (1-100); `deploys` action `list` takes empty input and lists approved catalog entries. Listing does not authorize writes or execution.

Consequential `machines` actions:

| Action | Exact input and safety |
| --- | --- |
| `create` | `display_name`, `ssh_host`, optional fixed `ssh_port`/`ssh_username`, optional independently verified `pinned_host_public_key`. |
| `update` | Exact `machine_id`, display and connection fields; preserves existing trust. |
| `delete` | Exact `machine_id`; removes the inventory record. |
| `host-key.clear` | Exact `machine_id`; explicit trust removal, not automatic mismatch recovery. |
| `host-key.replace` | Exact `machine_id` and independently verified `host_public_key`. |

The SSH identity is fixed to port 22 and user homelab. Unpinned hosts are not trusted automatically. Existing host-key mismatches fail closed; connection updates do not reset trust.

Consequential `deploys` action `run` accepts only discovered approved `deploy_id` and exact `machine_id` UUID. Catalog entries determine availability, entrypoint, inventory, timeout, and runtime policy. Require target-specific execution approval even for a catalog entry described as read-only. No arbitrary commands, paths, inventory payloads, or bootstrap action are exposed. Acceptance or cancellation is not proof of completion; report unknown outcomes and never retry automatically.

Both mixed tools take `action`, typed `input`, optional jq-compatible `filter`, and no help actions. Their tool-level annotations remain consequential; use action intent to distinguish listing from writes without claiming per-action OAuth enforcement.
