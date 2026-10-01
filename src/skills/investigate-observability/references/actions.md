# Grafana Actions

Discover definitions through resources: `homelab://grafana/dashboards`, `homelab://grafana/dashboards/{uid}`, `homelab://grafana/alert-rules`, and `homelab://grafana/recording-rules`. Collection input accepts the existing limits and dashboard query/tags/page fields.

Read-only `query` actions:

| Action | Selection and bounds |
| --- | --- |
| `grafana.logql.query` | `query`; instant `time` or paired `start`/`end`; `direction`, `limit` (1-5000). |
| `grafana.promql.query` | `query`; instant `time` or paired `start`/`end` and positive `step`; range at most 24 hours and 11,000 points. |
| `grafana.traceql.search` | `query`; optional paired `start`/`end`; `limit` (1-100), range at most 24 hours. |
| `grafana.profile.merge` | `selector`, `start`, `end`; `profile_type`, `max_nodes` (1-1000), range at most one hour. |
| `grafana.alert-instance.list` | Optional label `matchers`, `limit` (1-100). |
| `grafana.silence.list` | Optional `state` (active, pending, expired), `limit` (1-100). |

Read-only `query`: `grafana.render.dashboard` requires `uid`; `grafana.render.panel` also requires `panel_id`. Discover them with the dashboard collection and exact-UID resource. Optional `from`/`to`, `width` (320-2000), `height` (200-2000), `scale` (1-2), `theme`, `timezone`, and `variables` bound the image request. The result preserves a PNG image block alongside safe metadata.

Consequential `create`: only `grafana.silence.create`, requiring 1-20 label `matchers` (name, operator, value), positive `duration_seconds` capped at seven days, and `comment` capped at 512 UTF-8 bytes. This suppresses notifications; inspection intent does not authorize it. Inspect alert instances and current silences before the explicit decision, then inspect silences after acceptance or an unknown outcome. Do not automatically repeat a POST.

All tools take `action`, action-specific `input`, and optional jq-compatible `filter`. Filters select successful structured JSON directly and synchronize text; images and semantic errors remain intact. There are no help actions. Use the listed schema for exact fields; do not invent URLs or credentials.

Resource template descriptions supply input schemas. Resource parameters `input` (JSON object) and `filter` (jq expression) are percent-encoded query values; path identities must not also appear in input. Discover templates before constructing a URI.
