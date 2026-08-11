# Grafana Tools Shared Contract

## Status

This document defines implemented worktree behavior. Local tests cover all three generated Grafana tool surfaces and mock Grafana integration. The dashboard, rendering, and alerting expansion has not been deployed or exercised against live Grafana.

## Tool Surfaces

One authenticated MCP server exposes three Grafana progressive tools:

| Tool | Actions | MCP annotations |
| --- | --- | --- |
| `grafana_query` | `logql.query`, `promql.query`, `traceql.search`, `profile.merge`, `alert-rule.list`, `alert-instance.list`, `silence.list`, `dashboard.list`, `dashboard.get` | `readOnlyHint: true`, `destructiveHint: false`, `idempotentHint: true`, `openWorldHint: true` |
| `grafana_render` | `dashboard`, `panel` | `readOnlyHint: true`, `destructiveHint: false`, `idempotentHint: true`, `openWorldHint: true` |
| `grafana_exec` | `silence.create` | `readOnlyHint: false`, `destructiveHint: false`, `idempotentHint: false`, `openWorldHint: true` |

`grafana_exec` is separately advertised as operationally consequential. `silence.create` is not available through `grafana_query`, and `silence.list` and the other read actions are not available through `grafana_exec`.

Both generated top-level schemas accept `action`, action-dependent `input`, and an optional jq-compatible `filter`. Each tool also generates `help`, which takes no `input` and reports that tool's namespaces. Calling `help.<namespace>` reports the namespace's actions and input schemas. Unknown fields, tools, actions, invalid schemas, and invalid filters produce JSON-RPC errors.

For a schema-valid action that returns a successful semantic `McpToolResult`, `filter` applies to `structuredContent`. The exact filtered JSON value becomes `structuredContent` directly, including arrays, scalars, and null, without a `{ "result": ... }` wrapper. Text content is rewritten to the compact serialized filtered JSON so visible text and structured output agree; non-text content blocks, `_meta`, and extensions remain unchanged. Generated help and other legacy JSON actions retain `{ "result": <filtered-value> }` wrapping. Without a filter, every `grafana_query` success uses the complete normalized JSON as text and the same object as `structuredContent`. `grafana_render` retains its specialized text-plus-image response, and `silence.create` retains its concise acknowledgment containing the created silence ID. `isError: true` results preserve their complete original envelope without applying the filter.

## Authorization and Destination

The existing `mcp:use` scope authorizes every action on all three tools. There is no narrower read, render, or mutation scope, so every principal allowed to query can also render images and request silence creation.

All actions share one `GrafanaClient`, one configured Grafana origin, and one server-held Editor service-account token. The same credential reads dashboards, requests rendering, reads alerting state, and creates silences. The caller cannot choose the origin, token, API path, datasource, headers, or HTTP method.

The token is sent only as an upstream Bearer `Authorization` header. Redirects remain disabled so credentials never reach a redirect target.

## Shared Request Boundary

| Limit | Contract |
| --- | --- |
| Concurrent Grafana operations | Four across all three Grafana tools and actions. Permits are acquired immediately without waiting. |
| Operation timeout | 30 seconds after permit acquisition, covering dispatch and the complete response read. |
| Encoded URL | At most 8192 bytes after path join and query encoding. |
| Decoded response body | At most 4 MiB, with or without `Content-Length`. |

Rendering additionally has two immediate permits and a 25-second complete-operation timeout. It must acquire both a global and render permit; either capacity failure occurs before dispatch, and both permits are released on every completion or cancellation path. Render responses use the [separate image contract](../../grafana-render/spec/common.md).

The client releases its permit after success, failure, timeout, or cancellation. Capacity exhaustion returns before contacting Grafana. No action automatically retries an upstream request.

## Read Results and Errors

Every unfiltered `grafana_query` success returns one text item containing the complete normalized JSON and the same object-shaped value in `structuredContent`. Each focused action specification owns its normalized result contract. Reads never expose Grafana headers, credentials, datasource configuration, raw response wrappers, or unapproved upstream models.

Alert-rule and alert-instance label and annotation maps apply a conservative URL-field exclusion policy. After trimming surrounding whitespace, an entry is omitted when its case-insensitive key ends in `url`, or its value starts with `/` (including `//`), starts with an absolute URI scheme of the form `[A-Za-z][A-Za-z0-9+.-]*:`, or contains a non-empty Markdown link target of the form `](...)`. This deterministic policy applies equally to labels and annotations; ordinary text such as `API is failing` remains. It does not attempt to recognize every hostname or every possible URL representation.

Schema-valid read failures return one safe text item, `isError: true`, and an error object containing `code`, `message`, and `retryable`:

| Code | Condition | Retryable |
| --- | --- | --- |
| `invalid_arguments` | Semantic validation fails, or the encoded URL exceeds 8192 bytes. | `false` |
| `capacity_exhausted` | All four permits are in use. | `true` |
| `timeout` | A read exceeds 30 seconds. | `true` |
| `grafana_unauthorized` | Grafana returns 401 or 403. | `false` |
| `query_rejected` | Grafana returns another 4xx response, including 429. | `false` |
| `upstream_unavailable` | Transport fails, a redirect returns 3xx, or Grafana returns 5xx. | `true` |
| `invalid_response` | The body exceeds 4 MiB, contains invalid JSON, or violates action normalization. | `false` |

Shared messages are fixed:

| Code | Message |
| --- | --- |
| `capacity_exhausted` | `Grafana request capacity is currently exhausted.` |
| `timeout` | `The Grafana query timed out.` |
| `grafana_unauthorized` | `Grafana rejected the service credentials.` |
| `upstream_unavailable` | `Grafana is currently unavailable.` |
| `invalid_response` | `Grafana returned an invalid response.` |

Each read action supplies its own noun for `invalid_arguments` and `query_rejected`.

## Mutation Errors

`silence.create` uses the same semantic error envelope, but its post-dispatch failures distinguish explicit rejection from an uncertain outcome:

| Code | Condition | Retryable |
| --- | --- | --- |
| `invalid_arguments` | Input validation fails before dispatch. | `false` |
| `capacity_exhausted` | No permit is available, so Grafana is not contacted. | `true` |
| `grafana_unauthorized` | Grafana returns 401 or 403. | `false` |
| `mutation_rejected` | Grafana safely and explicitly rejects the mutation. | `false` |
| `mutation_outcome_unknown` | Transport, timeout, MCP cancellation, ambiguous status, response-read, JSON, or success-normalization failure occurs after dispatch may have begun. | `false` |

`mutation_rejected` uses `Grafana rejected the requested mutation.` An uncertain outcome uses `The Grafana mutation did not complete cleanly; its outcome may be uncertain. Check existing silences before retrying.` The tool does not retry automatically. Operators must use `silence.list` to inspect current silences before deciding whether to retry an uncertain request.

MCP cancellation of `silence.create` returns `mutation_outcome_unknown` because Grafana may already have accepted the POST. Read-action cancellation retains the generic `request cancelled` behavior.

All semantic messages omit matchers, comments, alert data, credentials, URLs, response bodies, query data, and transport details.

## Observability

Kuri generic MCP owns standard request spans and metrics. Homelab records only bounded Grafana-upstream attributes on `grafana.query` spans and request, duration, and in-flight metrics.

Dashboard inventory adds fixed actions `dashboard.list` and `dashboard.get`, modes `list` and `get`, and destination `grafana_dashboards`. Rendering uses fixed actions `dashboard` and `panel`, mode `render`, destination `grafana_rendering`, and outcomes `render_rejected` and `render_invalid_response`. Alerting retains its existing fixed values. Telemetry never emits UIDs, panel IDs, ranges, timezones, dimensions, variables, digests, images, matchers, comments, alert data, URLs, credentials, query data, or upstream bodies.

See the [observability architecture](../../architecture/observability.md) for metric names and the complete data-safety boundary.
