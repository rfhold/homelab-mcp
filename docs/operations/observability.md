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

Service-owned Grafana calls have the span hierarchy `mcp.server.request` -> `grafana.query` -> `http.client.request`. The final span covers the centralized Grafana HTTP attempt and uses the admitted `homelab_mcp::http_client` target. This boundary does not instrument dependency-owned OAuth or CIMD clients and does not require admitting Reqwest or middleware dependency targets.

`GrafanaClient::run` creates one `http.client.request` span before dispatch. The reqwest-tracing middleware uses that same span for W3C `traceparent` and active-context `tracestate` injection. A lifecycle guard keeps the span open through response body transfer and records `cancelled` if the request future drops before finalization.

The span records client kind, the controlled request method, response status when available, and one bounded outcome. A complete successful body transfer records `success`. A non-success HTTP response records `http_error`. A send or body transfer failure records `transport_error`. A response rejected by the body-size cap records `response_error`. Cancellation or timeout before finalization records `cancelled`. JSON parsing and schema validation occur after complete body transfer, so those application errors do not become transport errors.

HTTP 4xx and 5xx responses set OpenTelemetry error status from the HTTP status. Transport, response, and cancellation failures also set error status without raw details. Redirect responses retain `http_error` without OpenTelemetry error status. Transport failures do not record an error or cause string.

Grafana client spans omit server address, host, port, URL scheme, path, query, full URL, headers, body, query text, selectors, matchers, comments, tokens, user agent, and raw error or cause strings. The middleware does not change the current Grafana timeout, capacity, redirect, proxy, authorization, response, semantic error, or mutation uncertainty behavior.

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
| Parent traces do not connect | For inbound traces, confirm the caller sends valid W3C `traceparent` headers and proxies preserve them. For Grafana calls, confirm `http.client.request` is a child of `grafana.query` and Grafana receives its propagated `traceparent`. |
| Profiles do not appear | Confirm `HOMELAB_MCP_PYROSCOPE_URL` exists and egress reaches port 4040. Match the exact profile tags, then inspect Alloy and Pyroscope observability. |
| CPU or latency rises | Compare against a window without profiling. Remove the Pyroscope URL if 100 Hz sampling causes unacceptable overhead. |
| Grafana-upstream failures spike | Group upstream outcomes by action, mode, and datasource UID. Use `grafana.query` traces and correlated logs for the same interval. |
| Alert and recording-rule lists disagree | Confirm both actions use `grafana_alerting`, then inspect safe outcomes by action. The actions partition one fixed provisioning response by `record`. |
| Silence creation has an uncertain outcome | Do not retry automatically. Query current silences first; `mutation_outcome_unknown` means Grafana may have accepted the request before transport, timeout, or response validation failed. |

Never place tokens, authorization headers, full environment dumps, query text, profile selectors, alert matchers, comments, alert or recording-rule data, internal URLs, or upstream bodies in tickets or shared logs.

`RUST_LOG` affects JSON logs only. It cannot enable dependency target trees or bypass application redaction for URLs, bodies, upstream errors, or credentials.

Do not expect local application logs for runtime OTLP export or Pyroscope upload failures. Use backend freshness and telemetry-system health instead.
