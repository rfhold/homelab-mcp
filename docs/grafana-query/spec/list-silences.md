# List Silences Action Specification

## Status

This specification defines implemented worktree behavior for the read-only `grafana_query` action `silence.list`. Local validation and mock Grafana tests exist; authenticated preview calls and live silence-list API behavior remain unverified.

The [shared contract](common.md) owns authorization, tool annotations, fixed-destination transport, limits, errors, filtering, and telemetry.

## Input

| Field | Type | Required | Contract |
| --- | --- | --- | --- |
| `state` | string | No | Accepts `active`, `pending`, or `expired`. Omitting it returns every state. |
| `limit` | integer | No | Defaults to 50. Accepts 1 through 100. |

Unknown fields, unsupported states, or invalid limits produce `invalid_arguments` before Grafana is contacted.

## Grafana Request

The action sends `GET /api/alertmanager/grafana/api/v2/silences` to the fixed Grafana origin without caller-controlled query parameters. The caller cannot choose the path, method, origin, token, or headers.

Grafana does not receive the state or limit. The service validates and normalizes every returned entry, applies the optional state filter, and then applies the result limit while preserving upstream order.

## Success Result

An unfiltered success has this envelope:

```json
{
  "mode": "list",
  "result_type": "silences",
  "result": []
}
```

Each result contains exactly `silence_id`, `state`, `starts_at`, `ends_at`, `created_by`, `comment`, and `matchers`. Timestamps are validated and normalized to RFC3339 UTC. Each matcher contains exactly `name`, `operator`, and `value`; operators are normalized to `=`, `!=`, `=~`, or `!~` from Grafana's equality and regular-expression flags.

The list contains at most the validated limit. A silence ID, state, or timestamp uses at most 128 UTF-8 bytes. A creator uses at most 512 bytes, a comment uses at most 4096 bytes, and each silence has 1 through 20 matchers. Matcher names use at most 128 bytes and match `[A-Za-z_][A-Za-z0-9_]*`; values use at most 1024 UTF-8 bytes and may be empty.

Malformed, oversized, or unsupported entries produce `invalid_response`, including malformed entries that would have been filtered out or truncated. Raw Grafana wrappers, update timestamps, headers, credentials, and undocumented fields never enter the result.

## Recovery Use

After `silence.create` returns `mutation_outcome_unknown`, list `active` and, when the requested start may still be in the future, `pending` silences. Compare the normalized matchers, `created_by`, `comment`, `starts_at`, and `ends_at` with the attempted request before deciding whether to retry.

## Action Messages

`invalid_arguments` uses `The silence arguments are invalid.` `query_rejected` uses `Grafana rejected the silence query.` Other read failures follow the [shared error contract](common.md#read-results-and-errors).
