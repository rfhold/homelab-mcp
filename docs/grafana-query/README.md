# Grafana Tools

The repository implements three Grafana progressive tools on one authenticated MCP server. `grafana_query` owns ten bounded reads; `grafana_render` owns two bounded image reads; `grafana_exec` advertises operationally consequential writes and owns only `silence.create`.

Local tests cover generated schemas, annotations, dispatch, validation, limits, normalization, safe errors, and mock Grafana requests. The dashboard, rendering, alerting, and recording-rule expansion has not been deployed or exercised against live Grafana.

| Document | Covers |
| --- | --- |
| [Shared contract](spec/common.md) | Tool annotations, authorization, filtering, transport, limits, errors, and telemetry shared by every action. |
| [LogQL specification](spec/logql.md) | Inputs, modes, limits, Grafana routing, errors, and response constraints. |
| [PromQL specification](spec/promql.md) | Instant and range metrics queries through fixed Mimir routes. |
| [TraceQL specification](spec/traceql.md) | Bounded Tempo trace searches. |
| [Profiles specification](spec/profiles.md) | Bounded Pyroscope stacktrace merges. |
| [Alert-rule specification](spec/alert-rules.md) | Bounded normalized alert-rule reads that exclude recording rules. |
| [Recording-rule specification](spec/recording-rules.md) | Bounded normalized recording-rule reads from the shared provisioning route. |
| [Alert-instance specification](spec/alert-instances.md) | Matcher validation and bounded current-alert reads. |
| [Silence-list specification](spec/list-silences.md) | Bounded silence reads, state filtering, and recovery inspection. |
| [Dashboard-list specification](spec/list-dashboards.md) | Bounded dashboard search and normalized inventory. |
| [Dashboard-get specification](spec/get-dashboard.md) | Bounded dashboard, variable, and flattened panel inventory. |
| [Silence-creation specification](spec/create-silence.md) | Consequential silence creation, results, failure ambiguity, and operator guidance. |
| [Grafana render specification](../grafana-render/README.md) | Dashboard and panel PNG rendering contracts. |
| [Observability architecture](../architecture/observability.md) | Signal paths, identity, correlation, metrics, profiles, and data-safety boundaries. |
| [Observability operations](../operations/observability.md) | Runtime configuration, lifecycle, validation, and troubleshooting. |
| [Observability testing](../quality/observability-testing.md) | Local checks and required post-deployment evidence. |
