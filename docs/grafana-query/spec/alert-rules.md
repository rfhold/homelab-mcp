# Alert Rules Action Specification

## Status

This specification defines implemented worktree behavior for the read-only `grafana_query` action `alert-rule.list`. Local validation and mock Grafana tests exist; authenticated preview calls and live alert-rule API behavior remain unverified.

The [shared contract](common.md) owns authorization, tool annotations, fixed-destination transport, limits, errors, filtering, and telemetry.

## Input

| Field | Type | Required | Contract |
| --- | --- | --- | --- |
| `limit` | integer | No | Defaults to 50. Accepts 1 through 100. |

Unknown fields or values outside the limit produce `invalid_arguments` before Grafana is contacted.

## Grafana Request

The action sends `GET /api/v1/provisioning/alert-rules` to the fixed Grafana origin. The caller cannot alter the path, method, origin, token, or headers. The service applies the validated limit while normalizing the returned list.

## Success Result

An unfiltered success has this envelope:

```json
{
  "mode": "list",
  "result_type": "alert_rules",
  "result": []
}
```

Each result is a summary containing only `uid`, `title`, `folder_uid`, `rule_group`, `condition`, `no_data_state`, `exec_err_state`, `for`, `is_paused`, `labels`, and `annotations`. The list preserves upstream order and contains at most the validated limit.

Identifiers and state strings are bounded to 128 UTF-8 bytes; titles and rule groups are bounded to 512 bytes. Labels and annotations must be string maps with at most 64 entries, non-empty keys of at most 128 bytes, and values of at most 4096 bytes. Both maps use the exact conservative [URL-field exclusion policy](common.md#read-results-and-errors).

Malformed, oversized, or unsupported summaries produce `invalid_response`. URL-designated and URL-shaped map entries covered by that policy are omitted; the policy does not claim to detect every hostname or URL representation. Raw Grafana wrappers, headers, credentials, and rule query models never enter the result.

## Action Messages

`invalid_arguments` uses `The alert rule arguments are invalid.` `query_rejected` uses `Grafana rejected the alert rule query.` Other read failures follow the [shared error contract](common.md#read-results-and-errors).
