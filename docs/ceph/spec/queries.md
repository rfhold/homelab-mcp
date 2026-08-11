# Ceph Dashboard Query Specifications

## Query Actions

`ceph_query` exposes ten actions:

| Action | Contract |
| --- | --- |
| `cluster.list` | Return the configured `pantheon` and `romulus` catalog entries without contacting either Dashboard. |
| `status.get` | Return a bounded normalized summary of native cluster health and current service state from one exact cluster. |
| `metrics.summary` | Return a bounded normalized snapshot of current metrics exposed by one cluster's Dashboard API. |
| `osd.list` | Return bounded normalized OSD summaries. |
| `osd.get` | Return one exact OSD's normalized native state and approved metadata. |
| `osd.safe-to-destroy` | Ask the Dashboard whether one exact OSD ID is currently safe to destroy and return its normalized decision and bounded reason fields. |
| `device.list` | Return bounded normalized device summaries for one exact OSD. |
| `device.get` | Return one exact device attached to one exact OSD, with approved identity, location, daemon, and life-expectancy fields. |
| `flags.get` | Return the current state of MCP-allowlisted cluster flags. |
| `task.list` | Return bounded normalized current and recent Dashboard asynchronous tasks with stable follow-up identities. |

Every cluster-backed action requires one exact configured cluster selector. OSD selectors are non-negative integer IDs. Device actions also require the exact parent OSD ID. Device selectors and task identities must exactly match values returned by the same cluster's normalized results; callers cannot use them to construct a Dashboard path.

`osd.list`, `device.list`, and `task.list` accept an optional limit that defaults to 50 and ranges from 1 through 100. The task limit applies independently to executing and finished task groups. `status.get` returns at most 100 health checks. Truncation remains explicit.

## Current Metrics Only

`metrics.summary` reads only the current snapshot available from the Ceph Dashboard API. It does not query Prometheus, Grafana, Mimir, or another historical store. It does not accept a query expression, time range, step, datasource, or arbitrary metric name.

Historical Ceph metrics remain available through the existing Grafana and PromQL boundary when that datasource contains them. The Ceph feature does not duplicate or bypass that boundary.

## Flags

`flags.get` normalizes only these flags:

1. `noout`
2. `noin`
3. `noup`
4. `nodown`
5. `norebalance`
6. `norecover`
7. `nobackfill`
8. `noscrub`
9. `nodeep-scrub`

Unrecognized Dashboard flags do not expand the MCP contract or enter normalized output. The service exposes no flag mutation action.

## Task Identity and Output Safety

`task.list` filters Dashboard tasks to reviewed Ceph OSD task identities. It returns only bounded normalized identity and reviewed target metadata. It never exposes arbitrary metadata, exceptions, request bodies, commands, paths, or credentials.

All list actions report explicit truncation when upstream or normalized result bounds omit entries. Malformed selected data, oversized responses, and missing required identities fail safely rather than passing through raw Dashboard content.
