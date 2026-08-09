# Observability Operations

## Configuration

`HOMELAB_MCP_DEPLOYMENT_ENVIRONMENT` is required, even when OTLP export and profiling remain disabled.

| Variable | Requirement |
| --- | --- |
| `HOMELAB_MCP_DEPLOYMENT_ENVIRONMENT` | Required non-empty environment name. |
| `HOMELAB_MCP_K8S_NAMESPACE` | Optional resource attribute and profile tag. |
| `HOMELAB_MCP_K8S_POD_NAME` | Optional resource attribute. |
| `HOMELAB_MCP_K8S_POD_UID` | Optional source for `k8s.pod.uid` and `service.instance.id`. |
| `HOMELAB_MCP_PYROSCOPE_URL` | Optional credential-free HTTPS root origin. Enables 100 Hz CPU profiling. |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | Optional shared OTLP/HTTP base endpoint. Its presence enables trace and metric export. |
| `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` | Optional trace endpoint. Without the shared endpoint, the metric endpoint must also exist. |
| `OTEL_EXPORTER_OTLP_METRICS_ENDPOINT` | Optional metric endpoint. Without the shared endpoint, the trace endpoint must also exist. |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | Set to `http/protobuf` for the declared deployment. |
| `RUST_LOG` | Optional JSON log level filter for the approved `homelab_mcp` and `mcp` target trees. Invalid or absent configuration defaults both trees to `info`. It does not control OpenTelemetry trace admission. |

The declared deployment sends OTLP/HTTP to Alloy TCP port 4318. It sends profiles to TCP port 4040. Network policy permits both ports.

Do not print environment variables or rendered Secrets during diagnosis.

## Startup and Shutdown

With no OTLP endpoint variables, the service uses local telemetry mode. The shared endpoint enables both exports. Both signal-specific endpoints also enable export. Exactly one signal-specific endpoint without the shared endpoint fails startup.

Startup fails before the listener binds when endpoint mode, telemetry configuration, exporter construction, subscriber setup, or profiler startup fails. The Pyroscope URL must use HTTPS, contain no credentials, and have only the root path with no query or fragment.

An unreachable exporter does not stop request handling after startup. Trace, metric, and profile delivery can fail asynchronously. The hard target allowlist suppresses dependency diagnostics, so these failures produce no local application logs. Detect them through missing or stale backend data and Alloy or backend observability.

SIGTERM or Ctrl-C starts graceful HTTP shutdown. The service stops cleanup work before Pyroscope and OpenTelemetry shutdown. A dedicated standard thread runs the complete Pyroscope stop and shutdown sequence. Main waits at most ten seconds, then detaches a timed-out worker and continues. Meter and tracer shutdown each allow five seconds in sequence, for ten combined OpenTelemetry seconds. Provider shutdown performs the flush; no separate force-flush call exists.

The `Drop` fallback starts detached best-effort Pyroscope cleanup without a wait. Application startup and shutdown events remain local, but they contain only sanitized status and bounded outcomes.

If a request loses its task before a response, the metrics guard finalizes it as cancelled. Active request count returns to balance, and completion telemetry omits response status.

`/health` and `/ready` intentionally emit no HTTP request spans, metrics, or completion logs. Diagnose probe failures from Kubernetes probe status and endpoint behavior rather than expecting application request telemetry.

At 100 Hz, the profiler samples each process one hundred times per second. Watch CPU usage and request latency after rollout. Disable profiling by removing `HOMELAB_MCP_PYROSCOPE_URL` if overhead breaches the service budget.

## Validation

Use Grafana Explore after an authorized deployment. These examples contain no credentials.

Logs in Loki:

```logql
{service_name="homelab-mcp"} | json | deployment_environment_name="preview"
```

Correlated logs by a trace ID copied from Tempo:

```logql
{service_name="homelab-mcp"} | json | trace_id="<trace-id>"
```

HTTP request rate in Mimir:

```promql
sum by (http_route, http_outcome) (rate(http_server_request_count{service_name="homelab-mcp"}[5m]))
```

Grafana-upstream failures in Mimir:

```promql
sum by (action, outcome) (rate(homelab_mcp_grafana_upstream_requests_total{service_name="homelab-mcp",outcome!="success"}[5m]))
```

Service traces in Tempo:

```traceql
{ resource.service.name = "homelab-mcp" }
```

Select `homelab-mcp` and CPU profile type in Pyroscope. Filter by `service_namespace="homelab"` and the target `deployment_environment_name`.

Metric backend translation can replace dots with underscores and append `_total` to counters. Confirm final names through Grafana metric autocomplete.

## Troubleshooting

| Symptom | Checks |
| --- | --- |
| Process exits before listen | Check safe error text for missing deployment environment, incomplete signal-specific OTLP endpoints, an invalid Pyroscope origin, or profiler initialization failure. |
| JSON logs exist but traces do not | Do not adjust `RUST_LOG`; it controls JSON logs only. Confirm the missing span uses an approved target at `ERROR`, `WARN`, or `INFO`. Then confirm the shared endpoint or both signal-specific endpoints exist, protocol uses HTTP protobuf, and egress reaches Alloy port 4318. Inspect Alloy and Tempo observability. |
| Metrics do not appear | Confirm the same OTLP settings, wait for the periodic export interval, then inspect Alloy and Mimir observability. |
| Probe request telemetry does not appear | This is intentional for `/health` and `/ready`; use Kubernetes probe status and direct endpoint behavior. |
| Trace IDs do not appear in logs | Confirm the event occurs inside an instrumented HTTP or Grafana span. Startup events legitimately omit IDs. |
| Parent traces do not connect | Confirm the caller sends valid W3C `traceparent` headers. Proxies must preserve those headers. |
| Profiles do not appear | Confirm `HOMELAB_MCP_PYROSCOPE_URL` exists and egress reaches port 4040. Match the exact profile tags, then inspect Alloy and Pyroscope observability. |
| CPU or latency rises | Compare against a window without profiling. Remove the Pyroscope URL if 100 Hz sampling causes unacceptable overhead. |
| Grafana-upstream failures spike | Group upstream outcomes by action, mode, and datasource UID. Use `grafana.query` traces and correlated logs for the same interval. |

Never place tokens, authorization headers, full environment dumps, query text, or profile selectors in tickets or shared logs.

`RUST_LOG` affects JSON logs only. It cannot enable dependency target trees or bypass application redaction for URLs, bodies, upstream errors, or credentials.

Do not expect local application logs for runtime OTLP export or Pyroscope upload failures. Use backend freshness and telemetry-system health instead.
