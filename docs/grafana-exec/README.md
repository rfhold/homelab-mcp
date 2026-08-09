# Grafana Exec

This domain defines the planned progressive Grafana tool. No tool implementation exists yet.

The initial tool surface contains `grafana_exec` with one action, `logql`. A progressive tool keeps one stable MCP tool and selects behavior through an action field.

| Document | Covers |
| --- | --- |
| [LogQL specification](spec/logql.md) | Inputs, modes, limits, Grafana routing, errors, and response constraints. |
