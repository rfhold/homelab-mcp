# Ceph Actions

Read-only `ceph_query`:

| Action | Input |
| --- | --- |
| `cluster.list` | Empty input; configured catalog only. |
| `status.get`, `metrics.summary`, `flags.get` | Exact discovered `cluster`. |
| `osd.list`, `task.list` | `cluster`, optional `limit` (1-100). |
| `osd.get`, `osd.safe-to-destroy` | `cluster`, exact integer `osd_id`. |
| `device.list` | `cluster`, `osd_id`, optional `limit` (1-100). |
| `device.get` | `cluster`, `osd_id`, exact discovered `device_id`. |

Consequential `ceph_exec` requires `cluster` and exact `osd_id`:

| Action | Additional input |
| --- | --- |
| `osd.mark` | `state`: in, out, or down. |
| `osd.reweight` | Finite `weight` from 0 through 1. |
| `osd.scrub` | `kind`: normal or deep. |
| `osd.destroy`, `osd.purge` | `confirmation`: `destroy osd.<osd_id> on <cluster>` or `purge osd.<osd_id> on <cluster>`, respectively. |

Inspect health, flags, tasks, and the OSD before the explicit decision. Destructive actions run a fresh server-side safety check; a client check is evidence, not a substitute or authority. Preserve the distinction between destroy and purge. After any unknown outcome inspect the OSD and tasks, and do not automatically repeat the mutation. No `flags.set` action exists. Tools take `action`, typed `input`, optional jq-compatible `filter`; there are no help actions.
