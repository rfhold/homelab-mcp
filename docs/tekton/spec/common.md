# Tekton Tools Shared Contract

## Status

This document defines the locally implemented behavior. Rust and Pulumi tests provide local evidence; no deployment or live end-to-end verification has been performed.

## Tool Surfaces

Repository and workflow definitions are resources; run/task/log/status/wait reads use `query`, and dispatch/rerun/cancel use `execute`. The [uniform MCP interface](../../architecture/mcp-interface.md) owns exact public domain-prefixed names, schemas, resources, annotations, and routing. Focused specifications retain unqualified backend operation labels. No help actions or compatibility aliases exist.

Typed tool inputs and jq projections retain their existing semantics, synchronized text, image content, and unchanged semantic tool errors. Resource failures use JSON-RPC errors with safe semantic details. The same global authorization, fixed upstream operations, cancellation, and bounds apply.

Successful unfiltered tool results retain complete normalized JSON in text and structured content, including repository, workflow, run/task identities and mutation acceptance details. Resource results preserve the same normalized relationships as JSON content.

## Authorization

The global set `mcp:use kubernetes:read kubernetes:write inventory:read inventory:write inventory:host-trust deploy:read deploy:run` gates every action on resources and tools. No narrower Tekton read or mutation enforcement exists.

This choice lets every current MCP principal dispatch workflows, rerun runs, and cancel active runs. Each caller must make an explicit user decision before an exec call.

## Authority and Identity

Kubernetes PAC `Repository` custom resources in namespace `pipelines-as-code` define repository authority. Forgejo organization enumeration does not define or expand that authority.

Every external repository key uses exact canonical `org/repo` form. The [repository specification](repositories-workflows.md#repository-list) defines derivation, validation, duplicate handling, and the hard-cut alias policy.

PAC custom-resource names remain internal authority and adapter values. Callers cannot use them as selectors, and results cannot use them as relationship values.

Run and task IDs use exact `<namespace>/<name>` Kubernetes identities. Workflow IDs remain opaque `workflow/<base64url-sha256>` values.

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
