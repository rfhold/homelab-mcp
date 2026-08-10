# PromQL Action Specification

The read-only `promql.query` action executes Prometheus-compatible queries against fixed Mimir datasource UID `mimir`. The [shared contract](common.md) defines tool behavior, transport limits, and common errors.

## Input

| Field | Type | Required | Contract |
| --- | --- | --- | --- |
| `query` | string | Yes | Contains at least one non-whitespace character. |
| `start` | RFC3339 timestamp | Range only | Appears with `end` and `step`. |
| `end` | RFC3339 timestamp | Range only | Appears with `start` and `step`. |
| `step` | Prometheus duration | Range only | Unsigned integer segments in descending `y`, `w`, `d`, `h`, `m`, `s`, `ms` order, with a positive total. |
| `time` | RFC3339 timestamp | No | Applies only to instant mode. Omission asks Grafana for the current instant. |

Neither `start`, `end`, nor `step` selects instant mode. All three select range mode. Partial range fields, `time` in range mode, an empty query, or an invalid timestamp produces `invalid_arguments`.

A range accepts equal endpoints and spans at most 24 hours. It rejects `start` after `end`. The point formula `floor((end - start) / step) + 1` cannot exceed 11,000.

## Grafana Request

| Mode | Method and route |
| --- | --- |
| Instant | `GET /api/datasources/uid/mimir/resources/api/v1/query` |
| Range | `GET /api/datasources/uid/mimir/resources/api/v1/query_range` |

Instant requests send `query` and optional `time`. Range requests send `query`, `start`, `end`, and the unchanged validated `step`. RFC3339 values reach Grafana as UTC timestamps with nanosecond precision.

## Success Result

The normalized envelope has `mode` set to `instant` or `range`. It accepts these upstream result types:

| Result type | Normalized `result` |
| --- | --- |
| `matrix` | Array of `{ "metric": {<label>: <string>}, "values": [{"timestamp": <RFC3339>, "value": <string>}] }`. |
| `vector` | Array of `{ "metric": {<label>: <string>}, "value": {"timestamp": <RFC3339>, "value": <string>} }`. |
| `scalar` or `string` | `{ "timestamp": <RFC3339>, "value": <string> }`. |

The client requires upstream `status: "success"`, `data.resultType`, and `data.result`. It rejects unsupported types, non-string labels or values, and malformed samples as `invalid_response`.

## Action Messages

`invalid_arguments` uses `The PromQL arguments are invalid.` `query_rejected` uses `Grafana rejected the PromQL query.`
