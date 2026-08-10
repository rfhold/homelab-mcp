# Grafana Query Shared Contract

## Status

This document defines implemented worktree behavior. Local tests cover the generated tool surface and mock Grafana integration. Live Grafana evidence remains outstanding.

## Tool Surface

`#[mcp::progressive_server]` exposes one MCP tool named `grafana_query`. Its annotations declare read-only, non-destructive, idempotent, open-world behavior.

The generated top-level schema accepts:

| Field | Contract |
| --- | --- |
| `action` | Required. Accepts `help`, `logql`, `promql`, `traceql`, or `profiles`. |
| `input` | Required for domain actions and forbidden for `help`. Its schema depends on `action`. |
| `filter` | Optional jq-compatible string. Applies after successful action execution. |

The schema rejects unknown top-level fields. Every domain input schema also rejects unknown fields.

`help` returns descriptions, guidance, and generated input schemas for all four domain actions. An optional help filter applies to the generated help value.

For domain actions, `filter` applies only to `structuredContent` after execution. An object filter result becomes `structuredContent`. Any other result becomes `{ "result": <value> }`. Filtering preserves `content`, `isError`, `_meta`, and extensions.

Malformed JSON-RPC requests, unknown tools, unknown actions, invalid schemas, and invalid filters return JSON-RPC errors. Schema-valid action failures return semantic tool errors.

## Shared Request Boundary

All domain actions use one `GrafanaClient` and one Grafana origin. The caller cannot select an origin, datasource UID, credential, or authorization header.

The client sends the configured Viewer token only as an upstream Bearer `Authorization` header. Redirects remain disabled, so credentials never reach a redirect target.

| Limit | Contract |
| --- | --- |
| Concurrent Grafana operations | Four across all actions. Acquisition does not wait. |
| Operation timeout | 30 seconds after permit acquisition. Covers request dispatch and full response read. |
| Encoded URL | At most 8192 bytes after path join and query encoding. |
| Decoded response body | At most 4 MiB, with or without `Content-Length`. |

The client releases its permit after success, failure, timeout, or cancellation. Capacity exhaustion returns immediately and does not contact Grafana.

The 8192-byte URL cap applies to GET requests and the Profiles POST URL. Profiles input resides in the JSON body. No separate MCP-message or serialized output cap exists.

## Success Envelope

Every unfiltered success returns one short text item and object-shaped `structuredContent`:

```json
{
  "mode": "instant",
  "result_type": "vector",
  "result": []
}
```

The text states the mode, result type, and top-level item count. Object results count as one item.

Each action specification defines its modes, result types, and normalized `result`. The client never exposes Grafana headers, credentials, datasource configuration, or the Grafana response wrapper.

## Semantic Errors

Semantic failures return one safe text item, `isError: true`, and this envelope:

```json
{
  "error": {
    "code": "capacity_exhausted",
    "message": "Grafana query capacity is currently exhausted.",
    "retryable": true
  }
}
```

| Code | Condition | Retryable |
| --- | --- | --- |
| `invalid_arguments` | Semantic input validation fails, or the encoded URL exceeds 8192 bytes. | `false` |
| `capacity_exhausted` | All four permits remain in use. | `true` |
| `timeout` | The operation exceeds 30 seconds. | `true` |
| `grafana_unauthorized` | Grafana returns 401 or 403. | `false` |
| `query_rejected` | Grafana returns another 4xx response, including 429. | `false` |
| `upstream_unavailable` | Transport fails, a redirect returns 3xx, or Grafana returns 5xx. | `true` |
| `invalid_response` | The response exceeds 4 MiB, contains invalid JSON, or violates action normalization. | `false` |

Shared messages remain fixed:

| Code | Message |
| --- | --- |
| `capacity_exhausted` | `Grafana query capacity is currently exhausted.` |
| `timeout` | `The Grafana query timed out.` |
| `grafana_unauthorized` | `Grafana rejected the service credentials.` |
| `upstream_unavailable` | `Grafana is currently unavailable.` |
| `invalid_response` | `Grafana returned an invalid response.` |

Each action supplies its own noun in `invalid_arguments` and `query_rejected`. Error messages omit credentials, URLs, response bodies, query data, and transport details.

## Observability

Kuri generic MCP owns standard request spans and metrics. Homelab adds telemetry only for Grafana-upstream attempts: the `grafana.query` span and request, duration, and in-flight metrics record action, mode, fixed datasource UID, and a bounded outcome.

See the [observability architecture](../../architecture/observability.md) for metric names and attributes.
