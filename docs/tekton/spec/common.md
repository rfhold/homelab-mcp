# Tekton Tools Shared Contract

## Status

This document defines the locally implemented behavior. Rust and Pulumi tests provide local evidence; no deployment or live end-to-end verification has been performed.

## Tool Surfaces

The authenticated MCP server exposes two progressive tools:

| Tool | Actions | MCP annotations |
| --- | --- | --- |
| `tekton_query` | `repository.list`, `workflow.list`, `run.list`, `run.get`, `task.list`, `task.logs` | `readOnlyHint: true`, `destructiveHint: false`, `idempotentHint: true`, `openWorldHint: true` |
| `tekton_exec` | `workflow.dispatch`, `run.rerun`, `run.cancel` | `readOnlyHint: false`, `destructiveHint: true`, `idempotentHint: false`, `openWorldHint: true` |

Each tool provides progressive `help` actions, action-dependent `input`, and an optional jq-compatible `filter`. Filtering affects successful structured content only.

Read actions exist only on `tekton_query`. Mutation actions exist only on `tekton_exec`.

## Authorization

The current `mcp:use` scope authorizes every action on both tools. No narrower Tekton read or mutation scope exists.

This choice lets every current MCP principal dispatch workflows, rerun runs, and cancel active runs. Each caller must make an explicit user decision before an exec call.

## Authority and Identity

Kubernetes PAC `Repository` custom resources in namespace `pipelines-as-code` define repository authority. Forgejo organization enumeration does not define or expand that authority.

Repository results include the exact `pipelines-as-code/<name>` custom-resource identity. Run and task IDs use exact `<namespace>/<name>` Kubernetes identities.

Every run and task read or mutation validates ownership against an authorized PAC repository. A caller-supplied namespace, name, label, or relationship cannot bypass that validation.

The focused specifications define workflow identity and resource relationships. Results will use normalized allowlisted fields and will exclude raw Kubernetes, PAC, and Forgejo objects.

## Shared Boundaries

All actions enforce fixed limits before and during upstream work. Shared upstream limits are four concurrent requests, a 30-second timeout per request, and a 4 MiB decoded response body. Action-specific limits are defined in the focused specifications and generated schemas.

Partial failures will remain explicit when an action can safely return independent results. A result will identify omitted or failed units without exposing secret values or raw upstream bodies.

No action retries an upstream request automatically. The caller cannot choose an upstream origin, URL, route, method, headers, credential, Kubernetes namespace, or PAC custom-resource identity.

## Data Safety

MCP results, logs, traces, metrics, and errors will exclude MCP-held Forgejo tokens, PAC secrets, Kubernetes bearer tokens, authorization headers, request bodies, and internal service URLs.

Errors will use bounded codes and safe messages. They will not expose raw Kubernetes, PAC, Forgejo, or workload responses.

[`task.logs`](runs-tasks-logs.md#log-confidentiality) has a narrower guarantee. The service can redact MCP-held secrets, but it cannot reliably identify arbitrary workload secrets.

## Evidence Boundary

Local Rust and Pulumi tests verify the implemented contracts and declarations. They cannot verify cluster RBAC, PAC behavior, Forgejo behavior, secret projection, or mutation outcomes in a live environment.

Any preview read or mutation requires exact target-specific authority. Production remains unapplied and outside current verification.
