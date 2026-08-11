# Recording Rules Action Specification

## Status

This specification defines the canonical behavior for the read-only `grafana_query` action `recording-rule.list`. Repository-local validation and mock Grafana tests cover the action. An authenticated call against preview commit `798dd92` returned `invalid_response` with `limit: 1`. Grafana's authoritative response DTO omits empty labels, while the strict current normalizer rejects missing labels. This is the evidence-backed likely mismatch; the raw preview response was not captured. The approved label and synthetic-group normalization fixes have not been deployed or verified live.

The [shared contract](common.md) owns authorization, tool annotations, fixed-destination transport, limits, errors, filtering, and telemetry.

## Input

| Field | Type | Required | Contract |
| --- | --- | --- | --- |
| `limit` | integer | No | Defaults to 50. Accepts 1 through 100. |

Unknown fields or values outside the limit produce `invalid_arguments` before Grafana is contacted.

## Grafana Request and Classification

The action sends `GET /api/v1/provisioning/alert-rules` to the fixed Grafana origin. The caller cannot alter the path, method, origin, token, or headers.

Grafana returns alert rules and recording rules in one array. The service classifies each object by its `record` field:

- a missing or null `record` field identifies an alert rule;
- an object-valued `record` field identifies a recording rule; and
- any other `record` type produces `invalid_response`.

The service filters for recording rules before it applies the validated limit. It strictly normalizes only the selected recording rules up to that limit. Alert-rule details do not affect recording-rule normalization after classification.

## Success Result

An unfiltered success has this envelope:

```json
{
  "mode": "list",
  "result_type": "recording_rules",
  "result": []
}
```

Each result contains only `uid`, `title`, `folder_uid`, `rule_group`, `metric`, `source_ref`, `target_datasource_uid`, `is_paused`, and `labels`. `source_ref` comes from `record.from`. `target_datasource_uid` comes from `record.target_datasource_uid`; a missing, null, or empty value becomes null. The list preserves upstream category order and contains at most the validated limit.

`rule_group` follows the [shared rule-summary normalization](common.md#read-results-and-errors).

Identifiers, source references, and datasource UIDs are bounded to 128 UTF-8 bytes. Titles, rule groups, and metric names are bounded to 512 bytes. Missing or null `labels` normalize to an empty map. Object-valued `labels` must form a string map with at most 64 entries, non-empty keys of at most 128 bytes, and values of at most 4096 bytes. Labels use the exact conservative [URL-field exclusion policy](common.md#read-results-and-errors). Any other `labels` shape produces `invalid_response`.

Malformed, oversized, or unsupported selected recording summaries produce `invalid_response`. URL-designated and URL-shaped label entries covered by that policy are omitted. Expressions, query models, annotations, alert-rule details, datasource internals, raw Grafana wrappers, headers, credentials, and upstream responses never enter the result.

## Action Messages

`invalid_arguments` uses `The recording rule arguments are invalid.` `query_rejected` uses `Grafana rejected the recording rule query.` Other read failures follow the [shared error contract](common.md#read-results-and-errors).
