# Alert Rules Action Specification

## Status

This specification defines the canonical behavior for the read-only `grafana_query` action `alert-rule.list`. Local validation and mock Grafana tests exist. An authenticated call against preview commit `798dd92` succeeded with at least 100 entries. The approved synthetic-group normalization fix has not been deployed or verified live.

The [shared contract](common.md) owns authorization, tool annotations, fixed-destination transport, limits, errors, filtering, and telemetry.

## Input

| Field | Type | Required | Contract |
| --- | --- | --- | --- |
| `limit` | integer | No | Defaults to 50. Accepts 1 through 100. |

Unknown fields or values outside the limit produce `invalid_arguments` before Grafana is contacted.

## Grafana Request

The action sends `GET /api/v1/provisioning/alert-rules` to the fixed Grafana origin. The caller cannot alter the path, method, origin, token, or headers.

Grafana returns alert rules and recording rules in one array. The service classifies each object by its `record` field:

- a missing or null `record` field identifies an alert rule;
- an object-valued `record` field identifies a recording rule; and
- any other `record` type produces `invalid_response`.

The service filters for alert rules before it applies the validated limit. It strictly normalizes only the selected alert rules up to that limit. Recording-rule details do not affect alert-rule normalization after classification.

## Success Result

An unfiltered success has this envelope:

```json
{
  "mode": "list",
  "result_type": "alert_rules",
  "result": []
}
```

Each result is a summary containing only `uid`, `title`, `folder_uid`, `rule_group`, `condition`, `no_data_state`, `exec_err_state`, `for`, `is_paused`, `labels`, and `annotations`. Recording rules never enter the result. The list preserves upstream category order and contains at most the validated limit.

`rule_group` follows the [shared rule-summary normalization](common.md#read-results-and-errors).

Identifiers and state strings are bounded to 128 UTF-8 bytes; titles and rule groups are bounded to 512 bytes. Labels and annotations must be string maps with at most 64 entries, non-empty keys of at most 128 bytes, and values of at most 4096 bytes. Both maps use the exact conservative [URL-field exclusion policy](common.md#read-results-and-errors).

Malformed, oversized, or unsupported selected alert summaries produce `invalid_response`. URL-designated and URL-shaped map entries covered by that policy are omitted; the policy does not claim to detect every hostname or URL representation. Expressions, query models, recording-rule details, raw Grafana wrappers, headers, credentials, and datasource internals never enter the result.

## Action Messages

`invalid_arguments` uses `The alert rule arguments are invalid.` `query_rejected` uses `Grafana rejected the alert rule query.` Other read failures follow the [shared error contract](common.md#read-results-and-errors).
