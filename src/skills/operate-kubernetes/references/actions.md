# Kubernetes Actions

Discover configured clusters at `homelab://kubernetes/clusters` and approved kinds at `homelab://kubernetes/capabilities/{cluster}` (optional kinds subset). Read-only `query` actions `kubernetes.resource_list` and `kubernetes.resource_get` retain the existing exact scope, labels, name and limit fields. Follow the schema's cluster-scoped versus namespaced branches; never invent support for a kind.

`kubernetes.pod_logs` requires `cluster`, `namespace`, `pod`, current discovered `pod_uid`, `container`, and `instance` (current or previous). Optional `tail_lines` (1-1000) and `max_bytes` (1-262144) keep reads bounded. Refresh resource identity before reading logs if the Pod has been replaced.

Consequential `execute` (restart, scale, suspend), `create` (trigger), and `destroy` (Pod deletion) actions all require exact `cluster`, `namespace`, and `name`, with optional `dry_run`:

| Action | Additional selection |
| --- | --- |
| `kubernetes.workload_restart` | `kind`: deployment, stateful_set, daemon_set. |
| `kubernetes.workload_scale` | `kind`: deployment or stateful_set; bounded `replicas` (0-1000). |
| `kubernetes.cronjob_suspend` | `suspended`: true to suspend, false to resume. |
| `kubernetes.cronjob_trigger` | Server generates the Job name from the exact CronJob. |
| `kubernetes.pod_delete` | Ordinary exact Pod deletion; no force-delete or bulk selector. |

Dry runs are explicit requests, not implicit permission to mutate. Verify with `kubernetes.resource_get` and bounded related resource/event reads. Accepted writes may not have converged. Unknown outcomes are inspected, never automatically retried. Tools take `action`, typed `input`, optional jq-compatible `filter`, and no help actions.

Resource template descriptions supply input schemas. Resource parameters `input` (JSON object) and `filter` (jq expression) are percent-encoded query values; path identities must not also appear in input. Discover templates before constructing a URI.
