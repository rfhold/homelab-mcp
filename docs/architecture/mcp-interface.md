# Homelab MCP Interface

## Discovery And Reads

Configuration and definition catalogs use `resources/list`, `resources/templates/list`, and `resources/read`. Live state, logs, runs, tasks, waits, observability queries, and rendered images remain tools. This is a hard cutover from the former domain-specific tools; no aliases or generated help actions remain.

| Collection URI | Content |
| --- | --- |
| `homelab://kubernetes/clusters` | Configured cluster identities; no live objects. |
| `homelab://ceph/clusters` | Configured Ceph cluster identities. |
| `homelab://grafana/dashboards` | Bounded normalized dashboard inventory. |
| `homelab://grafana/alert-rules` | Alert-rule definitions, not active alerts. |
| `homelab://grafana/recording-rules` | Recording-rule definitions. |
| `homelab://tekton/repositories` | Authorized PAC repository definitions. |
| `homelab://tekton/workflows` | Workflow definitions, not runs. |
| `homelab://machines` | Machine inventory and public host-trust status. |
| `homelab://deploys` | Approved deploy definitions, not execution. |

Each collection has a `{?input,filter}` template. The two identity templates are `homelab://kubernetes/capabilities/{cluster}{?input,filter}` and `homelab://grafana/dashboards/{uid}{?input,filter}`. Template descriptions contain JSON input schemas, including existing limits. Path identities are percent-encoded once; they must not be repeated in `input`.

Dashboard collection entries add `resource_uri` for identities that pass the exact dashboard reader's validation. Kubernetes cluster entries add `capabilities_uri`. Each link uses the canonical `homelab://` identity route with percent-encoded path components. The resource adapter preserves all original fields and adds links before optional projection. Other collections emit no unsupported item links. These additions affect resource contents only; integration handlers retain their original output.

`input` is a percent-encoded JSON object and defaults to `{}`. `filter` is an optional percent-encoded jq expression over normalized JSON. Unknown or duplicate parameters, non-object input, invalid identities, and URIs over 8192 bytes are rejected. Fragment identifiers are not supported. Listings are complete, without pagination; cursors are rejected.

For example, read `homelab://grafana/dashboards?input=%7B%22limit%22%3A1%7D&filter=.result` to request at most one dashboard and project its normalized result list. Resource contents echo the requested URI and use `application/json`; projections use the pinned MCP JSON-result primitive, which wraps non-object values in a `result` field. Catalog errors use JSON-RPC errors and retain the safe semantic error code, message, and retryability in their message. Skill files retain their separate `skill://` identities and exact authored Markdown bytes; see [MCP Skills](mcp-skills.md).

## Uniform Tools

`tools/list` advertises exactly four tools and 46 actions. Every tool accepts `action`, typed action-specific `input`, and optional jq-compatible `filter`. Public action names include a domain prefix. No editable source text is exposed, so there is no `edit` tool; structured machine fields and operational commands are not document edits.

| Tool | Annotations |
| --- | --- |
| `query` | Read-only, non-destructive, idempotent, open-world. |
| `create` | Non-read-only, non-destructive, non-idempotent, open-world. |
| `execute` | Non-read-only, destructive, non-idempotent, open-world. |
| `destroy` | Non-read-only, destructive, non-idempotent, open-world. |

| Tool | Domain | Exact actions |
| --- | --- | --- |
| `query` | Grafana | `grafana.logql.query`, `grafana.promql.query`, `grafana.traceql.search`, `grafana.profile.merge`, `grafana.alert-instance.list`, `grafana.silence.list`, `grafana.render.dashboard`, `grafana.render.panel` |
| `query` | Tekton | `tekton.run.list`, `tekton.run.get`, `tekton.run.status`, `tekton.run.wait`, `tekton.task.list`, `tekton.task.logs` |
| `query` | Kubernetes | `kubernetes.resource_list`, `kubernetes.resource_get`, `kubernetes.pod_logs` |
| `query` | Ceph | `ceph.status.get`, `ceph.metrics.summary`, `ceph.osd.list`, `ceph.osd.get`, `ceph.osd.safe-to-destroy`, `ceph.device.list`, `ceph.device.get`, `ceph.flags.get`, `ceph.task.list` |
| `create` | Grafana, Kubernetes, machines | `grafana.silence.create`, `kubernetes.cronjob_trigger`, `machine.create` |
| `execute` | Tekton | `tekton.workflow.dispatch`, `tekton.run.rerun`, `tekton.run.cancel` |
| `execute` | Kubernetes | `kubernetes.workload_restart`, `kubernetes.workload_scale`, `kubernetes.cronjob_suspend` |
| `execute` | Ceph | `ceph.osd.mark`, `ceph.osd.reweight`, `ceph.osd.scrub` |
| `execute` | Machines, deploys | `machine.update`, `machine.host-key.clear`, `machine.host-key.replace`, `deploy.run` |
| `destroy` | Kubernetes, Ceph, machines | `kubernetes.pod_delete`, `ceph.osd.destroy`, `ceph.osd.purge`, `machine.delete` |

Create actions create actual objects. Workflow dispatch and rerun remain execution because their contract is operational acceptance, not a synchronously created inventory object. Tool filters preserve direct structured-content projection, synchronized text, images, and unchanged `isError` envelopes. Unknown tool/action combinations and invalid input shapes return JSON-RPC errors. Focused integration specifications may refer to unqualified backend operation names; the table here owns public MCP routing. Telemetry keeps its existing fixed backend labels.

## Safety And Ownership

The same hosted authorization, complete global scope set, Origin checks, `ServerContext`, cancellation, progress reporting, concurrency limits, deadlines, output bounds, normalization, and redaction protect resources and tools. This routing change adds no per-action authorization and does not resolve existing production authorization blockers.

Read or discovery intent does not authorize mutations. Every consequential action requires an explicit exact-target decision. Ceph destroy and purge retain explicit confirmation and a fresh same-call safe-to-destroy check. Tekton cancellation retains its resourceVersion guard. Machine connection updates preserve trust; host-key clearing and replacement remain separate explicit operations. Unknown outcomes require inspection rather than automatic retries. No raw Kubernetes or Grafana manifests, arbitrary commands, secrets, or writable resource documents are exposed.

[`src/mcp/tools.rs`](../../src/mcp/tools.rs) builds schemas and dispatch with public primitives from the unchanged immutable MCP Git dependency. [`src/mcp/resources.rs`](../../src/mcp/resources.rs) serves resource catalogs and forwards calls to existing integration handlers in [`src/mcp.rs`](../../src/mcp.rs). No unpublished macro API or local path dependency is required.
