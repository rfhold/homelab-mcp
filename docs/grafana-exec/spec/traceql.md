# TraceQL Action Specification

The `traceql` action searches fixed Tempo datasource UID `tempo`. The [shared contract](common.md) defines tool behavior, transport limits, and common errors.

## Input

| Field | Type | Required | Contract |
| --- | --- | --- | --- |
| `query` | string | Yes | Contains at least one non-whitespace character. |
| `start` | RFC3339 timestamp | No | Appears with `end`. |
| `end` | RFC3339 timestamp | No | Appears with `start`. |
| `limit` | integer | No | Defaults to 20. Accepts 1 through 100. |

The optional range accepts equal endpoints and spans at most 24 hours. It rejects partial ranges and `start` after `end`.

## Grafana Request

The action sends `GET /api/datasources/proxy/uid/tempo/api/search`. Query parameters contain `q`, `limit`, and optional `start` and `end` from `DateTime::timestamp()` as Unix seconds.

## Success Result

```json
{
  "mode": "search",
  "result_type": "traces",
  "result": [],
  "metrics": {}
}
```

The client requires a top-level object with a `traces` array. It preserves trace entries and truncates the array to the validated limit in upstream order.

If upstream `metrics` contains an object, the client preserves it under `metrics`. It omits absent or non-object metrics. Other top-level fields do not enter the result.

## Action Messages

`invalid_arguments` uses `The TraceQL arguments are invalid.` `query_rejected` uses `Grafana rejected the TraceQL query.`
