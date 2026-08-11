# Run, Task, and Log Specifications

## Shared Ownership

`run.list`, `run.get`, `run.wait`, `task.list`, and `task.logs` will return only resources that belong to an authorized repository. Each action will validate Kubernetes ownership and PAC relationships before it returns data.

Repository selectors and relationships follow the [canonical repository key contract](repositories-workflows.md#repository-list).

Run and task IDs will use exact `<namespace>/<name>` identities. The service will reject ambiguous names and identities without a namespace.

Lists use reverse chronology and exact identity as the deterministic tie breaker. Run and task list limits default to 50 and accept values from 1 through 100.

## Runs

`run.list` is the newest-run lookup surface. It returns bounded `PipelineRun` summaries for an authorized repository in reverse chronology.

The action accepts optional exact `workflow`, normalized `status`, and `revision` filters. The optional `branch` filter treats `main` and `refs/heads/main` as equivalent. Revision and branch remain distinct: `revision` selects an exact commit revision, while `branch` selects an equivalent branch label.

An explicit `limit: 1` returns the newest matching run among inspected source objects. The output includes `source_truncated` and `result_truncated`, plus aggregate `truncated` when either specific flag is true. If `source_truncated` is true, the newest guarantee applies only to inspected runs.

`run.get` returns one owned `PipelineRun` by its exact namespace-qualified ID.

## Run Wait

`run.wait` is read-only and requires an exact namespace-qualified `run_id`. Its optional timeout defaults to 60 seconds and accepts values from 1 through 300 seconds.

The action returns immediately when the selected run is terminal. Otherwise, the server polls every two seconds until the run reaches a terminal state or the deadline expires.

A terminal result observed during the condition wait contains the normalized run and `timed_out: false`. At the condition deadline, the server performs one final ownership-matching read, bounded by the shared 30-second request timeout. A successful final read returns that freshly authorized normalized run with `timed_out: true`, even if it has just become terminal. Failed authorization or a failed final read returns the corresponding safe tool error rather than stale state.

The server permits at most four concurrent waits, separate from the shared upstream HTTP request capacity. A wait remains cancellable and releases all permits when it ends. It does not hold an upstream HTTP permit while it sleeps.

Before output, the action revalidates run ownership and the authorized canonical repository relationship. Its output contains no task details or logs. After a failed run, callers use `task.list` and `task.logs` for diagnosis.

Run output will use an explicit allowlist. It can include identity, repository relationship, workflow relationship, revision, lifecycle timestamps, condition status, reason, and bounded parameter summaries.

Run output will exclude raw objects, managed fields, annotations, arbitrary labels, pod specifications, service-account tokens, credentials, and unapproved status extensions.

## Tasks

`task.list` resolves an owned `PipelineRun` before it lists related `TaskRun` resources. It requires both the exact PAC labels and a TaskRun owner reference matching the PipelineRun name and UID, then returns reverse-chronological normalized summaries with exact namespace-qualified task IDs.

Task output will use an explicit allowlist. It can include identity, parent run identity, pipeline task name, lifecycle timestamps, condition status, reason, and bounded step summaries.

The action rejects a task whose labels or immutable owner relationship do not match the selected run and authorized repository.

## Logs

`task.logs` resolves an owned task and verifies that the resolved pod has an exact TaskRun owner reference before reading logs. It can select an allowlisted step from that task.

The log tail defaults to 200 lines and accepts 1 through 1,000. Returned bytes default to 65,536 and accept 1 through 262,144 across at most eight steps. Per-step metadata conservatively reports possible tail and byte truncation. The result includes `steps_omitted` and an aggregate `truncated` flag.

Run and task source lists follow Kubernetes continuation tokens for at most 500 objects. Task lists expose `truncated: true` when their source or result ceiling omits objects. Run lists expose the specific and aggregate truncation fields defined above.

Results will identify the exact task and selected step. They will not expose pod specifications, environment variables, Kubernetes tokens, or raw API responses.

## Log Confidentiality

The service redacts every configured secret value held by the MCP runtime before it returns logs. This includes the database URL and password, OIDC client secret, OAuth wrapping keys, Grafana token, Forgejo token, PAC input secret, and the projected Kubernetes token used for that log request.

The service cannot reliably recognize arbitrary workload secrets in task output. A workload can print credentials unknown to the MCP runtime.

Therefore, `task.logs` retains a residual confidentiality risk after redaction. Callers must treat returned logs as sensitive and avoid external publication without review.
