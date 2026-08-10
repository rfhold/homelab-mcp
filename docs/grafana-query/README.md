# Grafana Query

The repository implements one progressive, read-only `grafana_query` tool. Local tests cover validation, dispatch, limits, normalization, and mock Grafana requests.

The generated tool provides `help`, jq-compatible output filters, and four domain actions. Live Grafana verification for this worktree remains outstanding.

| Document | Covers |
| --- | --- |
| [Shared contract](spec/common.md) | Tool shape, filtering, transport, limits, success envelopes, and errors shared by every action. |
| [LogQL specification](spec/logql.md) | Inputs, modes, limits, Grafana routing, errors, and response constraints. |
| [PromQL specification](spec/promql.md) | Instant and range metrics queries through fixed Mimir routes. |
| [TraceQL specification](spec/traceql.md) | Bounded Tempo trace searches. |
| [Profiles specification](spec/profiles.md) | Bounded Pyroscope stacktrace merges. |
| [Observability architecture](../architecture/observability.md) | Signal paths, identity, correlation, metrics, profiles, and data-safety boundaries. |
| [Observability operations](../operations/observability.md) | Runtime configuration, lifecycle, validation, and troubleshooting. |
| [Observability testing](../quality/observability-testing.md) | Local checks and required post-deployment evidence. |
