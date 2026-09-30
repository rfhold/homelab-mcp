# Tekton Actions

Read-only `tekton_query`:

| Action | Input |
| --- | --- |
| `repository.list` | Optional `limit` (1-100). |
| `workflow.list` | Optional exact discovered `repository`, `limit` (1-200). |
| `run.list` | Exact `repository`; optional `branch`, normalized `revision`, `status`, workflow definition name, `limit` (1-100). |
| `run.get`, `run.status` | Exact `run_id` in namespace/name form. |
| `run.wait` | Exact `run_id`, optional `timeout_seconds` (1-300). |
| `task.list` | Exact `run_id`, optional `limit` (1-100). |
| `task.logs` | Exact `run_id` and discovered `task_id`; optional discovered `step`, `tail_lines` (1-1000), `max_bytes` (1-262144). |

Consequential `tekton_exec`:

| Action | Explicit decision |
| --- | --- |
| `workflow.dispatch` | Exact discovered repository and opaque workflow identity, `ref`, and optional string `params`. Only incoming-enabled workflows are dispatchable. |
| `run.rerun` | Exact owned `run_id`; uses its fixed PAC incoming route. |
| `run.cancel` | Exact active owned `run_id`. |

Read results preserve identities needed for follow-up. Acceptance is not successful execution. Use `run.list`, `run.status`, or bounded `run.wait` to verify. Unknown outcomes require inspection, not a repeated mutation. Tools take `action`, typed `input`, and optional jq-compatible `filter`; no help actions or arbitrary pipeline payloads exist.
