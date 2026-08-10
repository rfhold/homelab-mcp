# Run, Task, and Log Specifications

## Shared Ownership

`run.list`, `run.get`, `task.list`, and `task.logs` will return only resources that belong to an authorized PAC repository. Each action will validate Kubernetes ownership and PAC relationships before it returns data.

Run and task IDs will use exact `<namespace>/<name>` identities. The service will reject ambiguous names and identities without a namespace.

Lists use reverse chronology and exact identity as the deterministic tie breaker. Run and task list limits default to 50 and accept values from 1 through 100.

## Runs

`run.list` will return bounded `PipelineRun` summaries for an authorized repository. `run.get` will return one owned `PipelineRun` by its exact namespace-qualified ID.

Run output will use an explicit allowlist. It can include identity, repository relationship, workflow relationship, revision, lifecycle timestamps, condition status, reason, and bounded parameter summaries.

Run output will exclude raw objects, managed fields, annotations, arbitrary labels, pod specifications, service-account tokens, credentials, and unapproved status extensions.

## Tasks

`task.list` resolves an owned `PipelineRun` before it lists related `TaskRun` resources. It requires both the exact PAC labels and a TaskRun owner reference matching the PipelineRun name and UID, then returns reverse-chronological normalized summaries with exact namespace-qualified task IDs.

Task output will use an explicit allowlist. It can include identity, parent run identity, pipeline task name, lifecycle timestamps, condition status, reason, and bounded step summaries.

The action rejects a task whose labels or immutable owner relationship do not match the selected run and authorized repository.

## Logs

`task.logs` resolves an owned task and verifies that the resolved pod has an exact TaskRun owner reference before reading logs. It can select an allowlisted step from that task.

The log tail defaults to 200 lines and accepts 1 through 1,000. Returned bytes default to 65,536 and accept 1 through 262,144 across at most eight steps. Per-step metadata conservatively reports possible tail and byte truncation. The result includes `steps_omitted` and an aggregate `truncated` flag.

Kubernetes lists follow continuation tokens for at most 500 source objects. Repository authority fails closed if that ceiling is exceeded. Run and task lists expose `truncated: true` when the source or result ceiling omits objects.

Results will identify the exact task and selected step. They will not expose pod specifications, environment variables, Kubernetes tokens, or raw API responses.

## Log Confidentiality

The service redacts every configured secret value held by the MCP runtime before it returns logs. This includes the database URL and password, OIDC client secret, OAuth wrapping keys, Grafana token, Forgejo token, PAC input secret, and the projected Kubernetes token used for that log request.

The service cannot reliably recognize arbitrary workload secrets in task output. A workload can print credentials unknown to the MCP runtime.

Therefore, `task.logs` retains a residual confidentiality risk after redaction. Callers must treat returned logs as sensitive and avoid external publication without review.
