# Profiles Action Specification

The read-only `profile.merge` action merges stacktraces from fixed Pyroscope datasource UID `pyroscope`. The [shared contract](common.md) defines tool behavior, transport limits, and common errors.

## Input

| Field | Type | Required | Contract |
| --- | --- | --- | --- |
| `selector` | string | Yes | Contains at least one non-whitespace character. |
| `start` | RFC3339 timestamp | Yes | Inclusive range start. |
| `end` | RFC3339 timestamp | Yes | Inclusive range end. |
| `profile_type` | string | No | Non-empty. Defaults to `process_cpu:cpu:nanoseconds:cpu:nanoseconds`. |
| `max_nodes` | integer | No | Defaults to 256. Accepts 1 through 1000. |

The range accepts equal endpoints and spans at most one hour. It rejects `start` after `end`.

## Grafana Request

The action sends this request:

```text
POST /api/datasources/proxy/uid/pyroscope/querier.v1.QuerierService/SelectMergeStacktraces
```

The JSON body uses Grafana's expected field names:

```json
{
  "profileTypeID": "process_cpu:cpu:nanoseconds:cpu:nanoseconds",
  "labelSelector": "{service_name=\"homelab-mcp\"}",
  "start": 1786269600000,
  "end": 1786273200000,
  "maxNodes": 256
}
```

`start` and `end` use Unix milliseconds. The action sends no URL query parameters.

## Success Result

```json
{
  "mode": "range",
  "result_type": "stacktraces",
  "result": {
    "flamegraph": {
      "names": [],
      "levels": [],
      "total": "0",
      "maxSelf": "0"
    }
  }
}
```

The upstream response must contain exactly one top-level field named `flamegraph`. Its value must be an object with `names` and `levels` arrays plus string-valued `total` and `maxSelf` fields. The flamegraph object can contain other fields.

The client preserves the complete validated response under `result`. Any other successful JSON response produces `invalid_response`. The decoded 4 MiB response cap bounds this payload.

## Action Messages

`invalid_arguments` uses `The profile arguments are invalid.` `query_rejected` uses `Grafana rejected the profile query.`
