# Tekton Actions

Discover definitions through `homelab://tekton/repositories` (optional limit 1-100) and `homelab://tekton/workflows` (optional exact repository and limit 1-200).

Read-only `query`:

| Action | Input |
| --- | --- |
| `tekton.run.list` | Exact `repository`; optional `branch`, normalized `revision`, `status`, workflow definition name, `limit` (1-100). |
| `tekton.run.get`, `tekton.run.status` | Exact `run_id` in namespace/name form. |
| `tekton.run.wait` | Exact `run_id`, optional `timeout_seconds` (1-300). |
| `tekton.task.list` | Exact `run_id`, optional `limit` (1-100). |
| `tekton.task.logs` | Exact `run_id` and discovered `task_id`; optional discovered `step`, `tail_lines` (1-1000), `max_bytes` (1-262144). |

Consequential `execute`:

| Action | Explicit decision |
| --- | --- |
| `tekton.workflow.dispatch` | Exact discovered repository and opaque workflow identity, `ref`, and optional string `params`. Only incoming-enabled workflows are dispatchable. |
| `tekton.run.rerun` | Exact owned `run_id`; uses its fixed PAC incoming route. |
| `tekton.run.cancel` | Exact active owned `run_id`; server preserves the resourceVersion guard. |

Read results preserve identities needed for follow-up. Acceptance is not successful execution. Use `tekton.run.list`, `tekton.run.status`, or bounded `tekton.run.wait` to verify. Unknown outcomes require inspection, not a repeated mutation. Tools take `action`, typed `input`, and optional jq-compatible `filter`; no help actions or arbitrary pipeline payloads exist.

Resource template descriptions supply input schemas. Resource parameters `input` (JSON object) and `filter` (jq expression) are percent-encoded query values; path identities must not also appear in input. Discover templates before constructing a URI.
