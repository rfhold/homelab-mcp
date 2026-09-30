# Kubernetes Actions

Read-only `kubernetes_query`: `cluster_list` takes an empty input and discovers configured cluster names; `capability_list` takes `cluster` and optional approved `kinds`. `resource_list` takes `cluster`, approved `kind`, optional `labels` and `limit`, and `namespace` for namespaced kinds. `resource_get` takes an exact name with the same scope selection. Follow the schema's cluster-scoped versus namespaced branches; never invent support for a kind.

`pod_logs` requires `cluster`, `namespace`, `pod`, current discovered `pod_uid`, `container`, and `instance` (current or previous). Optional `tail_lines` (1-1000) and `max_bytes` (1-262144) keep reads bounded. Refresh resource identity before reading logs if the Pod has been replaced.

Consequential `kubernetes_exec` actions all require exact `cluster`, `namespace`, and `name`, with optional `dry_run`:

| Action | Additional selection |
| --- | --- |
| `workload_restart` | `kind`: deployment, stateful_set, daemon_set. |
| `workload_scale` | `kind`: deployment or stateful_set; bounded `replicas` (0-1000). |
| `cronjob_suspend` | `suspended`: true to suspend, false to resume. |
| `cronjob_trigger` | Server generates the Job name from the exact CronJob. |
| `pod_delete` | Ordinary exact Pod deletion; no force-delete or bulk selector. |

Dry runs are explicit requests, not implicit permission to mutate. Verify with `resource_get` and bounded related resource/event reads. Accepted writes may not have converged. Unknown outcomes are inspected, never automatically retried. Tools take `action`, typed `input`, optional jq-compatible `filter`, and no help actions.
