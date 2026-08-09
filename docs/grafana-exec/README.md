# Grafana Exec

The repository implements the progressive Grafana tool. It is covered by local validation and mock HTTP tests but has not been tested against live Grafana or deployed to preview.

`#[mcp::progressive_server]` generates one read-only `grafana_exec` tool. It generates help, filtering, and schema behavior around one domain action, `logql`.

| Document | Covers |
| --- | --- |
| [LogQL specification](spec/logql.md) | Inputs, modes, limits, Grafana routing, errors, and response constraints. |
