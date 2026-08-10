# LogQL Action Specification

## Status

This specification defines the implemented worktree behavior for the `logql.query` action. Local tests cover its validation, normalized responses, limits, error mapping, and mock HTTP integration. This observability revision has no coordinator-confirmed preview deployment or authenticated live Grafana query.

## Tool Surface

`#[mcp::progressive_server]` generates the read-only MCP tool `grafana_query`. The [feature index](../README.md) lists its seven actions and the separately advertised `grafana_exec` tool.

The macro also generates action `help`, the filter behavior, and the tool schema. A help call takes this shape:

```json
{
  "action": "help",
  "filter": ".actions"
}
```

Help takes no `input`. Its structured output lists all seven `grafana_query` actions with their descriptions, guidance, and generated input schemas.

The [shared contract](common.md) defines generated tool behavior, shared transport limits, and common error mapping.

The optional top-level `filter` is a jq-compatible string. For semantic action output, it applies only to `structuredContent` after action execution.

A LogQL call takes this nested shape:

```json
{
  "action": "logql.query",
  "input": {
    "query": "{job=\"example\"} |= \"error\"",
    "start": "2026-08-09T10:00:00Z",
    "end": "2026-08-09T11:00:00Z",
    "direction": "backward",
    "limit": 1000
  },
  "filter": ".result"
}
```

The tool schema must reject unknown top-level and `input` fields. `input` is required for `logql.query` and forbidden for `help`.

Without `filter`, successful `logql.query` output uses the stable envelope below and preserves its original summary text. With `filter`, the generated progressive framework applies the expression only to successful `structuredContent`.

The exact filtered JSON value becomes `structuredContent` directly, including arrays, scalars, and null, without a `{ "result": ... }` wrapper. The framework rewrites text content to the compact serialized filtered JSON while preserving non-text content blocks, `_meta`, and extensions. It leaves `isError: true` results completely unchanged and does not apply their filters.

## LogQL Input

| Field | Type | Required | Contract |
| --- | --- | --- | --- |
| `query` | string | Yes | Must contain at least one non-whitespace character. |
| `start` | RFC3339 timestamp | Range only | Must appear with `end`. |
| `end` | RFC3339 timestamp | Range only | Must appear with `start`. |
| `time` | RFC3339 timestamp | No | Applies only to instant mode. Omission asks Grafana for the current instant. |
| `direction` | `forward` or `backward` | No | Applies only to range mode. Defaults to `backward`. |
| `limit` | integer | No | Defaults to 1000. Valid values are 1 through 5000. |

The service must parse every timestamp as RFC3339 before it contacts Grafana. It must preserve the represented instant and send Grafana the corresponding time value.

## Mode Selection

- Both `start` and `end` select range mode.
- Neither `start` nor `end` selects instant mode.
- One range endpoint without the other produces `invalid_arguments`.
- `time` with range mode produces `invalid_arguments`.
- `direction` with instant mode produces `invalid_arguments`.
- A range longer than 24 hours produces `invalid_arguments`.
- A `start` value after `end` produces `invalid_arguments`.

Equal range endpoints are valid. Validation must finish before concurrency acquisition or any Grafana request.

## Resource Limits

| Limit | Value |
| --- | --- |
| Default requested line limit | 1000 |
| Maximum query limit | 5000 |
| Maximum range | 24 hours |
| Grafana request timeout | 30 seconds |
| Service-wide concurrency across both tools and all actions | 4 |
| Maximum encoded request URL | 8192 bytes |
| Maximum decoded response body | 4 MiB |

The service must attempt concurrency acquisition without waiting. If all four permits are in use, it must fail with retryable `capacity_exhausted`.

The 30-second deadline covers the complete Grafana request and response read. Every completion path must release its concurrency permit.

The client rejects an encoded request URL above 8192 bytes before dispatch. It rejects a decoded response body above 4 MiB as `invalid_response`. No separate MCP-message or serialized output cap exists.

## Grafana Routing

Instant mode uses `/api/datasources/proxy/uid/loki/loki/api/v1/query`. Range mode uses `/api/datasources/proxy/uid/loki/loki/api/v1/query_range`.

Both modes must use fixed datasource UID `loki`. The service must call Grafana's proxy only and must not call Loki directly.

Both modes send the validated `limit` as Grafana's fixed query `limit` parameter.

The caller cannot select a Grafana URL, Loki URL, datasource UID, credential, or authorization header. The service sends its Editor service-account token only in the upstream `Authorization` header.

The Grafana HTTP client must disable redirects. It must not forward credentials to a redirect target.

## Success Result

A successful unfiltered call returns one short text content item and object-shaped `structuredContent`. The text states the mode, result type, and item count.

For a successful filtered call, the macro stores the exact direct filter output in `structuredContent` and rewrites visible text to the same value's compact JSON serialization. It preserves non-text content blocks, `_meta`, and extensions. Filtering never changes the Grafana request or line limit; an unfiltered call retains the original semantic summary described above.

`structuredContent` has this stable envelope:

```json
{
  "mode": "range",
  "result_type": "streams",
  "result": [],
  "stats": {
    "bytes_processed": 0,
    "lines_processed": 0,
    "entries_returned": 0,
    "execution_time_ms": 0
  }
}
```

`mode` is `instant` or `range`. `result_type` is `streams`, `matrix`, `vector`, or `scalar`.

The optional `stats` object can contain only the four fields shown above. The service omits unavailable fields and discards unknown upstream statistics.

The normalized `result` shape depends on `result_type`:

| Result type | Stable `result` shape |
| --- | --- |
| `streams` | Array of `{ "stream": {<label>: <value>}, "values": [{"timestamp": <RFC3339>, "line": <string>}] }`. |
| `matrix` | Array of `{ "metric": {<label>: <value>}, "values": [{"timestamp": <RFC3339>, "value": <string>}] }`. |
| `vector` | Array of `{ "metric": {<label>: <value>}, "value": {"timestamp": <RFC3339>, "value": <string>} }`. |
| `scalar` | `{ "timestamp": <RFC3339>, "value": <string> }`. |

Label maps use strings. Samples preserve upstream values as strings and normalize sample timestamps to RFC3339.

For stream results, the service deterministically truncates aggregate returned entries to the validated limit when Grafana overreturns. The bound applies only to returned log lines.

Matrix, vector, and scalar results remain supported. The service does not describe their metric samples as log lines.

The service must not expose Grafana's response wrapper, headers, datasource details, or credentials.

## Tool Errors

Well-formed `logql.query` calls with semantic validation or execution failures return a semantic `McpToolResult` with `isError: true`. The domain action returns that result directly with a short safe message and this stable envelope:

```json
{
  "error": {
    "code": "capacity_exhausted",
    "message": "Grafana request capacity is currently exhausted.",
    "retryable": true
  }
}
```

The seven stable semantic errors follow the [shared read error contract](common.md#read-results-and-errors). LogQL uses these action-specific messages:

| Code | Safe message | Condition | Retryable |
| --- | --- | --- | --- |
| `invalid_arguments` | `The LogQL arguments are invalid.` | Invalid field value or invalid instant/range combination. | `false` |
| `capacity_exhausted` | `Grafana request capacity is currently exhausted.` | All four shared permits are in use. The service fails immediately. | `true` |
| `timeout` | `The Grafana query timed out.` | The Grafana operation exceeds 30 seconds. | `true` |
| `grafana_unauthorized` | `Grafana rejected the service credentials.` | Grafana returns an authentication or authorization failure. | `false` |
| `query_rejected` | `Grafana rejected the LogQL query.` | Grafana rejects the LogQL query or request parameters. | `false` |
| `upstream_unavailable` | `Grafana is currently unavailable.` | Grafana transport fails or Grafana returns a retryable server failure. | `true` |
| `invalid_response` | `Grafana returned an invalid response.` | Grafana returns malformed or unsupported data. | `false` |

Messages must not include credentials, internal URLs, response bodies, query data, or low-level transport details.

Malformed JSON-RPC requests, unknown tools, unknown actions, schema-invalid tool shapes, and invalid filters use JSON-RPC errors. They do not use the semantic tool-error envelope.

Schema-valid values that violate LogQL combinations or bounds use `invalid_arguments`. Examples include an empty query, limit zero, and a lone range endpoint.
