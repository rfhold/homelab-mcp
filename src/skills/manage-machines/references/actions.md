# Machine And Deploy Actions

Read inventory through `homelab://machines` with optional input `limit` (1-100), and approved deploy definitions through `homelab://deploys` with empty input. Listing does not authorize writes or execution.

Consequential tools: `create` for `machine.create`, `execute` for update and host trust, and `destroy` for `machine.delete`:

| Action | Exact input and safety |
| --- | --- |
| `machine.create` | `display_name`, `ssh_host`, optional fixed `ssh_port`/`ssh_username`, optional independently verified `pinned_host_public_key`. |
| `machine.update` | Exact `machine_id`, display and connection fields; preserves existing trust. |
| `machine.delete` | Exact `machine_id`; removes the inventory record. |
| `machine.host-key.clear` | Exact `machine_id`; explicit trust removal, not automatic mismatch recovery. |
| `machine.host-key.replace` | Exact `machine_id` and independently verified `host_public_key`. |

The SSH identity is fixed to port 22 and user homelab. Unpinned hosts are not trusted automatically. Existing host-key mismatches fail closed; connection updates do not reset trust.

Consequential `execute` action `deploy.run` accepts only discovered approved `deploy_id` and exact `machine_id` UUID. Catalog entries determine availability, entrypoint, inventory, timeout, and runtime policy. Require target-specific execution approval even for a catalog entry described as read-only. No arbitrary commands, paths, inventory payloads, or bootstrap action are exposed. Acceptance or cancellation is not proof of completion; report unknown outcomes and never retry automatically.

Uniform tools take `action`, typed `input`, optional jq-compatible `filter`, and no help actions. Resources are discovery only; no editable text or per-action OAuth enforcement is provided.

Resource template descriptions supply input schemas. Resource parameters `input` (JSON object) and `filter` (jq expression) are percent-encoded query values; path identities must not also appear in input. Discover templates before constructing a URI.
