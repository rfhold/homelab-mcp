# Grafana Tools

The repository implements two progressive tools on one authenticated MCP server. `grafana_query` owns seven bounded reads; `grafana_exec` advertises operationally consequential writes and owns only `silence.create`.

Local tests cover generated schemas, annotations, dispatch, validation, limits, normalization, safe errors, and mock Grafana requests. The alerting revision has not been deployed or exercised against live Grafana.

| Document | Covers |
| --- | --- |
| [Shared contract](spec/common.md) | Tool annotations, authorization, filtering, transport, limits, errors, and telemetry shared by every action. |
| [LogQL specification](spec/logql.md) | Inputs, modes, limits, Grafana routing, errors, and response constraints. |
| [PromQL specification](spec/promql.md) | Instant and range metrics queries through fixed Mimir routes. |
| [TraceQL specification](spec/traceql.md) | Bounded Tempo trace searches. |
| [Profiles specification](spec/profiles.md) | Bounded Pyroscope stacktrace merges. |
| [Alert-rule specification](spec/alert-rules.md) | Bounded normalized Grafana alert-rule reads. |
| [Alert-instance specification](spec/alert-instances.md) | Matcher validation and bounded current-alert reads. |
| [Silence-list specification](spec/list-silences.md) | Bounded silence reads, state filtering, and recovery inspection. |
| [Silence-creation specification](spec/create-silence.md) | Consequential silence creation, results, failure ambiguity, and operator guidance. |
| [Observability architecture](../architecture/observability.md) | Signal paths, identity, correlation, metrics, profiles, and data-safety boundaries. |
| [Observability operations](../operations/observability.md) | Runtime configuration, lifecycle, validation, and troubleshooting. |
| [Observability testing](../quality/observability-testing.md) | Local checks and required post-deployment evidence. |
