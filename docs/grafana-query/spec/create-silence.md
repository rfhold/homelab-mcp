# Create Silence Action Specification

## Status

This specification defines implemented worktree behavior for `grafana_exec` action `silence.create`. Local validation and mock Grafana tests exist. The alerting revision has not been deployed; authenticated preview calls, live silence API behavior, and operation of the promoted Editor permission remain unverified.

`grafana_exec` advertises this action as non-read-only, non-destructive, non-idempotent, open-world, and operationally consequential. The [shared contract](common.md) owns the all-`mcp:use` authorization boundary, fixed destination, resource limits, filtering, errors, and telemetry.

## Effect

A silence suppresses notifications for matching alert instances during its interval. It does not stop alert-rule evaluation, delete alert data, or resolve the underlying condition.

## Input

| Field | Type | Required | Contract |
| --- | --- | --- | --- |
| `matchers` | array | Yes | Contains 1 through 20 matchers. |
| `duration_seconds` | integer | Yes | Accepts 1 through 604800 seconds. |
| `comment` | string | Yes | Contains non-whitespace text and uses at most 512 UTF-8 bytes. |

Each matcher has exactly `name`, `operator`, and `value`. Operators are `=`, `!=`, `=~`, and `!~`; names match `[A-Za-z_][A-Za-z0-9_]*` and use at most 128 bytes; values use at most 1024 UTF-8 bytes. Unknown fields or invalid values produce `invalid_arguments` before Grafana is contacted.

## Grafana Request

The action starts the silence immediately and computes its end from `duration_seconds`. It sends `POST /api/alertmanager/grafana/api/v2/silences` to the fixed Grafana origin with the validated matchers, computed `startsAt` and `endsAt`, the operator comment, and fixed `createdBy` value `homelab-mcp`.

The caller cannot select the origin, token, path, method, headers, `createdBy`, or an arbitrary start time. The service performs one request attempt and never retries automatically.

## Success Result

A successful result contains exactly:

```json
{
  "silence_id": "silence-123",
  "starts_at": "2026-08-10T12:00:00.000000000Z",
  "ends_at": "2026-08-10T13:00:00.000000000Z"
}
```

The result excludes matchers, comment, `createdBy`, request data, Grafana wrappers, headers, credentials, and internal URLs.

## Failure and Retry Guidance

An explicit safe Grafana rejection returns non-retryable `mutation_rejected`. A timeout, transport failure, ambiguous response status, response-read failure, malformed JSON, or malformed success response returns non-retryable `mutation_outcome_unknown` because Grafana may have created the silence.

For `mutation_outcome_unknown`, use [`silence.list`](list-silences.md) to inspect current silences before deciding whether to retry. Compare the matchers, creator, comment, and interval with the attempted request. Do not retry solely because the call returned an error. MCP cancellation after dispatch also returns `mutation_outcome_unknown` because Grafana may have accepted the request.

The exact mutation errors and safe messages are defined by the [shared mutation error contract](common.md#mutation-errors).
