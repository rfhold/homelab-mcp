# Grafana Actions

Read-only `grafana_query` actions:

| Action | Selection and bounds |
| --- | --- |
| `logql.query` | `query`; instant `time` or paired `start`/`end`; `direction`, `limit` (1-5000). |
| `promql.query` | `query`; instant `time` or paired `start`/`end` and positive `step`; range at most 24 hours and 11,000 points. |
| `traceql.search` | `query`; optional paired `start`/`end`; `limit` (1-100), range at most 24 hours. |
| `profile.merge` | `selector`, `start`, `end`; `profile_type`, `max_nodes` (1-1000), range at most one hour. |
| `alert-rule.list`, `recording-rule.list` | `limit` (1-100). |
| `alert-instance.list` | Optional label `matchers`, `limit` (1-100). |
| `silence.list` | Optional `state` (active, pending, expired), `limit` (1-100). |
| `dashboard.list` | Optional `query`, `tags`, one-based `page`, `limit` (1-100). |
| `dashboard.get` | Exact `uid` discovered from the list. |

Read-only `grafana_render`: `dashboard` requires `uid`; `panel` also requires `panel_id`. Discover them with `dashboard.list` and `dashboard.get`. Optional `from`/`to`, `width` (320-2000), `height` (200-2000), `scale` (1-2), `theme`, `timezone`, and `variables` bound the image request. The result preserves a PNG image block alongside safe metadata.

Consequential `grafana_exec`: only `silence.create`, requiring 1-20 label `matchers` (name, operator, value), positive `duration_seconds` capped at seven days, and `comment` capped at 512 UTF-8 bytes. This suppresses notifications; inspection intent does not authorize it. Inspect alert instances and current silences before the explicit decision, then inspect silences after acceptance or an unknown outcome. Do not automatically repeat a POST.

All tools take `action`, action-specific `input`, and optional jq-compatible `filter`. Filters select successful structured JSON directly and synchronize text; images and semantic errors remain intact. There are no help actions. Use the listed schema for exact fields; do not invent URLs or credentials.
