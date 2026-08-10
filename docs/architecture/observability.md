# Observability Architecture

## Scope

The runtime emits correlated traces, metrics, JSON logs, and CPU profiles. The host instruments non-probe HTTP traffic and Grafana-upstream operations. Kuri generic MCP owns standard MCP request instrumentation.

## Signal Paths

| Signal | Runtime path | Backend path |
| --- | --- | --- |
| Traces | OpenTelemetry batch exporter over OTLP/HTTP protobuf | Alloy port 4318, then the configured trace backend |
| Metrics | OpenTelemetry periodic exporter over OTLP/HTTP protobuf | Alloy port 4318, then the configured metrics backend |
| Logs | One JSON object per stdout line | Kubernetes log collection, then Loki |
| CPU profiles | Pyroscope Rust agent over HTTP | Alloy or Pyroscope port 4040, then Pyroscope |

The runtime does not configure an OTLP log exporter. Kubernetes already owns stdout collection. A second log exporter would duplicate records and create two delivery paths.

## Identity

Every OTLP trace and metric resource has these attributes:

| Attribute | Value |
| --- | --- |
| `service.name` | `homelab-mcp` |
| `service.namespace` | `homelab` |
| `deployment.environment.name` | Required runtime environment value |
| `k8s.namespace.name` | Pod namespace, when present |
| `k8s.pod.name` | Pod name, when present |
| `k8s.pod.uid` | Pod UID, when present |
| `service.instance.id` | Pod UID, when present |

Pulumi supplies Kubernetes values through the downward API. The runtime uses the pod UID for both `k8s.pod.uid` and `service.instance.id`. Resource identity comes from application configuration, not caller data.

## Traces and Correlation

The tracer uses `AlwaysOn` sampling. Every locally created span receives a sampled trace when OTLP export remains enabled.

The global W3C Trace Context propagator extracts inbound HTTP `traceparent` and `tracestate` headers. The HTTP server span adopts that remote context.

The JSON formatter adds lowercase hexadecimal `trace_id` and `span_id` fields when an active OpenTelemetry span exists. Events outside a span omit both fields.

HTTP spans record method, stable route, and outcome. Completed requests also record response status and error status for 5xx responses. Cancelled requests record error status without a response status. `/health` and `/ready` intentionally bypass this layer and emit no request spans, request metrics, or completion logs. Grafana-upstream `grafana.query` spans record bounded action, mode, destination, and outcome values for reads and writes.

OpenTelemetry trace admission is fixed. It accepts `ERROR`, `WARN`, and `INFO` spans and events only for `homelab_mcp`, `homelab_mcp::*`, `mcp`, and `mcp::*` targets. It rejects `DEBUG`, `TRACE`, dependency, and lookalike targets and does not consult `RUST_LOG`. The JSON layer applies the same target allowlist with `RUST_LOG`; invalid or absent configuration defaults both target trees to `INFO`.

## Metric Inventory

| Metric | Unit | Attributes |
| --- | --- | --- |
| `http.server.request.count` | count | `http.request.method`, `http.route`, `http.outcome`, plus `http.response.status_code` when a response exists |
| `http.server.active_requests` | count | `http.request.method`, `http.route` |
| `http.server.request.duration` | seconds | Same attributes as `http.server.request.count` |
| `mcp.server.request.count` | count | Protocol request identity and bounded outcome attributes owned by Kuri generic MCP |
| `mcp.server.request.duration` | seconds | Same protocol and outcome attributes as the generic MCP request count |
| `mcp.server.request.in_flight` | count | Protocol request identity attributes owned by Kuri generic MCP |
| `homelab_mcp.grafana.upstream.requests` | count | `action`, `mode`, `datasource_uid`, `outcome` |
| `homelab_mcp.grafana.upstream.duration` | seconds | Same attributes as upstream requests |
| `homelab_mcp.grafana.upstream.in_flight` | count | `action`, `mode`, `datasource_uid` |

The pinned Kuri generic MCP revision provides the MCP metrics. Homelab does not provide substitute MCP action spans or metrics.

HTTP route values use matched templates or bounded fallback classes. Action, mode, datasource, and outcome values pass through fixed allowlists. Alerting adds actions `alert_rules`, `alert_instances`, and `create_silence`, modes `list` and `create`, destination `grafana_alerting`, and outcomes `mutation_rejected` and `mutation_outcome_unknown`. Probe routes are intentionally absent from all HTTP request telemetry.

HTTP methods use canonical uppercase values for `GET`, `HEAD`, `POST`, `PUT`, `PATCH`, `DELETE`, `OPTIONS`, `CONNECT`, and `TRACE`. Every extension method becomes `OTHER` before span, metric, or log creation.

The HTTP request guard decrements `http.server.active_requests` on every completion path. Cancellation records request count, duration, and one JSON completion event with `http.outcome=cancelled`. It does not invent `http.response.status_code`.

## Profiles

The profiler samples process CPU stacks at 100 Hz. Its application name is `homelab-mcp`.

Profiles use these bounded tags:

| Tag | Source |
| --- | --- |
| `service_namespace` | Fixed `homelab` value |
| `deployment_environment_name` | Required runtime environment value |
| `namespace` | Kubernetes namespace, when present |

The agent does not add pod names, pod UIDs, user values, query text, or a second `service_name` tag. This policy limits cardinality and avoids Pyroscope label conflicts.

## Data Safety

Instrumentation excludes authorization headers, tokens, request bodies, query strings, LogQL, PromQL, TraceQL, profile selectors, alert matchers, silence comments, alert data, internal URLs, upstream errors, and upstream response bodies.

HTTP fallback routes collapse unknown identifiers. Semantic tool errors expose fixed safe text. Grafana-upstream metrics and spans use fixed datasource UIDs and bounded outcomes. The JSON target allowlist prevents permissive `RUST_LOG` directives from exposing dependency URL, body, or error events.

The allowlist intentionally suppresses exporter and profiler dependency diagnostics. Runtime export and upload failures do not create local application logs. Missing or stale backend data and Alloy or backend observability provide failure evidence. Application-owned startup and shutdown events remain sanitized and available locally.

## Lifecycle

Telemetry configuration loads before application configuration. Invalid required telemetry configuration stops startup.

OTLP export activates when `OTEL_EXPORTER_OTLP_ENDPOINT` exists. It also activates when both signal-specific trace and metric endpoints exist. Exactly one signal-specific endpoint without the shared endpoint stops startup. With no endpoint variables, local telemetry mode emits JSON logs without OTLP tracer or meter providers.

The profiler activates only when `HOMELAB_MCP_PYROSCOPE_URL` contains a credential-free HTTPS root origin. Invalid configuration, profiler construction, or profiler startup failure stops service startup.

On shutdown, the server drains first. A dedicated standard thread performs the complete Pyroscope stop and shutdown sequence. The main thread waits at most ten seconds. After timeout, it detaches the worker and continues with OpenTelemetry shutdown. Meter shutdown then flushes within five seconds. Tracer shutdown follows and flushes within five seconds. The two sequential OpenTelemetry limits allow ten seconds combined; no separate force-flush step exists.

If normal guard shutdown does not run, `Drop` starts detached best-effort Pyroscope cleanup and does not wait. Pyroscope shutdown logs expose only sanitized `completed`, `failed`, or `timed_out` outcomes.
