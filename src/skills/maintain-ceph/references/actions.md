# Ceph Actions

Discover configured clusters with `homelab://ceph/clusters`.

Read-only `query`:

| Action | Input |
| --- | --- |
| `ceph.status.get`, `ceph.metrics.summary`, `ceph.flags.get` | Exact discovered `cluster`. |
| `ceph.osd.list`, `ceph.task.list` | `cluster`, optional `limit` (1-100). |
| `ceph.osd.get`, `ceph.osd.safe-to-destroy` | `cluster`, exact integer `osd_id`. |
| `ceph.device.list` | `cluster`, `osd_id`, optional `limit` (1-100). |
| `ceph.device.get` | `cluster`, `osd_id`, exact discovered `device_id`. |

Consequential `execute` (mark, reweight, scrub) and `destroy` (destroy, purge) require `cluster` and exact `osd_id`:

| Action | Additional input |
| --- | --- |
| `ceph.osd.mark` | `state`: in, out, or down. |
| `ceph.osd.reweight` | Finite `weight` from 0 through 1. |
| `ceph.osd.scrub` | `kind`: normal or deep. |
| `ceph.osd.destroy`, `ceph.osd.purge` | `confirmation`: `destroy osd.<osd_id> on <cluster>` or `purge osd.<osd_id> on <cluster>`, respectively. |

Inspect health, flags, tasks, and the OSD before the explicit decision. Destructive actions run a fresh same-call server-side safety check; a client check is evidence, not a substitute or authority. Preserve the distinction between destroy and purge. After any unknown outcome inspect the OSD and tasks, and do not automatically repeat the mutation. No `flags.set` action exists. Tools take `action`, typed `input`, optional jq-compatible `filter`; there are no help actions.

Resource template descriptions supply input schemas. Resource parameters `input` (JSON object) and `filter` (jq expression) are percent-encoded query values; path identities must not also appear in input. Discover templates before constructing a URI.
