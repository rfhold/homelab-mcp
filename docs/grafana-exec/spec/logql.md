# LogQL Action Specification

## Status

This specification defines intended behavior for `grafana_exec` action `logql`. The repository has no implementation.

## Purpose

The action will execute bounded LogQL through Grafana's datasource proxy. It will always use the fixed datasource UID `loki`.

Direct Loki requests and caller-selected datasource UIDs are out of scope.

## Input Contract

| Field | Type | Required | Contract |
| --- | --- | --- | --- |
| `action` | string | Yes | Must equal `logql`. |
| `query` | string | Yes | LogQL expression sent to Grafana after validation. |
| `start` | timestamp | No | Must appear with `end`; selects range mode. |
| `end` | timestamp | No | Must appear with `start`; selects range mode. |
| `time` | timestamp | No | Applies only to instant mode. |
| `direction` | enum | No | Applies only to range mode. |
| `limit` | integer | No | Defaults to 1000 and must not exceed 5000. |

The implementation must define accepted timestamp syntax and `direction` enum values before this specification becomes executable.

## Mode Selection

- Both `start` and `end` select range mode.
- Neither `start` nor `end` selects instant mode.
- One range endpoint without the other produces an invalid-arguments error.
- `time` with range mode produces an invalid-arguments error.
- `direction` with instant mode produces an invalid-arguments error.
- A range must not exceed 24 hours.
- A range must order `start` before or equal to `end`.

## Resource Limits

| Limit | Planned value |
| --- | --- |
| Default query limit | 1000 |
| Maximum query limit | 5000 |
| Maximum range | 24 hours |
| Grafana request timeout | 30 seconds |
| Maximum response body | 8 MiB |
| Service-wide Grafana query concurrency | 4 |

The service must reject invalid limits before a Grafana request. It must stop oversized responses and report a bounded tool error.

## Grafana Routing

Instant mode will use Grafana's instant-query API for datasource UID `loki`. Range mode will use Grafana's range-query API for the same datasource.

The server will authenticate with its Grafana Viewer service-account token. The action will not accept caller credentials or datasource selection.

## Output Contract

The action will return Grafana query data in an MCP tool result. The exact normalized result schema remains an implementation decision.

The result must distinguish successful query data from tool errors. It must not include the Grafana service-account token or internal authorization headers.

## Error Classes

The implementation will map these conditions to stable, safe tool errors:

- invalid arguments;
- concurrency capacity exhaustion;
- Grafana timeout;
- Grafana authentication or authorization failure;
- Grafana query rejection;
- Grafana transport failure;
- malformed Grafana response; and
- response cap exhaustion.

Exact error codes and retry guidance remain unresolved implementation details.
