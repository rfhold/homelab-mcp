# Alert Instances Action Specification

## Status

This specification defines implemented worktree behavior for the read-only `grafana_query.alert_instances` action. Local validation and mock Grafana tests exist; authenticated preview calls and live alert-instance API behavior remain unverified.

The [shared contract](common.md) owns authorization, tool annotations, fixed-destination transport, limits, errors, filtering, and telemetry.

## Input

| Field | Type | Required | Contract |
| --- | --- | --- | --- |
| `matchers` | array | No | Defaults to an empty array. Accepts at most 20 matchers. |
| `limit` | integer | No | Defaults to 50. Accepts 1 through 100. |

Each matcher has exactly `name`, `operator`, and `value`. `operator` accepts `=`, `!=`, `=~`, or `!~`. A name must match `[A-Za-z_][A-Za-z0-9_]*` and use at most 128 bytes. A value can be empty and uses at most 1024 UTF-8 bytes. Unknown fields or invalid values produce `invalid_arguments` before Grafana is contacted.

## Grafana Request

The action sends `GET /api/alertmanager/grafana/api/v2/alerts` to the fixed Grafana origin. The server converts each validated matcher into a repeated `filter` query parameter; callers cannot provide raw filters or choose the path, method, origin, token, or headers.

## Success Result

An unfiltered success has this envelope:

```json
{
  "mode": "list",
  "result_type": "alert_instances",
  "result": []
}
```

Each result contains only `fingerprint`, `starts_at`, `updated_at`, `ends_at`, `state`, `silenced`, `inhibited`, `labels`, and `annotations`. Timestamps are validated and normalized to RFC3339 UTC. `silenced` and `inhibited` report whether Grafana returned any corresponding status references. The list preserves upstream order and contains at most the validated limit.

Fingerprints, states, and source timestamps are bounded to 128 UTF-8 bytes. Labels and annotations use the shared safe-map normalization: at most 64 string entries, non-empty keys of at most 128 bytes, values of at most 4096 bytes, and the exact conservative [URL-field exclusion policy](common.md#read-results-and-errors).

Malformed, oversized, or unsupported alerts produce `invalid_response`. URL-designated and URL-shaped map entries covered by that policy are omitted; the policy does not claim to detect every hostname or URL representation. Raw Grafana wrappers, headers, credentials, and unapproved status fields never enter the result.

## Action Messages

`invalid_arguments` uses `The alert instance arguments are invalid.` `query_rejected` uses `Grafana rejected the alert instance query.` Other read failures follow the [shared error contract](common.md#read-results-and-errors).
