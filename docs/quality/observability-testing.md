# Observability Testing

## Current Evidence

The current worktree has local unit, schema, mock HTTP, and Pulumi mock coverage. This document does not claim that the observability revision reached preview.

## Local Commands

Run the Rust checks from the repository root with Rust 1.96:

```bash
cargo +1.96.0 fmt --all -- --check
cargo +1.96.0 check --locked --all-targets --all-features
cargo +1.96.0 clippy --locked --all-targets --all-features -- -D warnings
cargo +1.96.0 test --locked --all-features
```

Run the Pulumi checks from `infra/pulumi`:

```bash
bun install --frozen-lockfile
bun run build
bun test index.test.ts
```

For a documentation-only change, also run:

```bash
git diff --check -- docs/grafana-query docs/architecture/observability.md docs/operations/observability.md docs/quality/observability-testing.md
```

## Local Coverage

Tests cover these observability contracts:

- local OTLP mode, shared endpoint mode, paired signal-specific endpoints, and rejection of either signal-specific endpoint alone;
- required resource identity, pod UID mapping to `k8s.pod.uid` and `service.instance.id`, and `AlwaysOn` trace correlation;
- JSON trace and span IDs, bounded HTTP routes, and one safe completion event for instrumented routes;
- complete telemetry bypass for `/health` and `/ready`, while endpoint responses and normal-route instrumentation remain intact;
- cancellation-safe HTTP metrics, balanced active requests, bounded cancelled outcomes, and absent response status for cancellation;
- canonical standard HTTP methods and `OTHER` for extension methods;
- fixed OpenTelemetry admission for exact and prefixed `homelab_mcp` and `mcp` targets at `ERROR`, `WARN`, and `INFO`, independent of `RUST_LOG`;
- export of `mcp.server.request` as the parent of `grafana.query` when `RUST_LOG` omits the `mcp` tree, plus JSON target isolation under permissive levels;
- bounded and sanitized Pyroscope cleanup outcomes, including the ten-second timeout path;
- exact host HTTP and Grafana-upstream metric names plus bounded action, mode, datasource, and outcome labels;
- credential-free HTTPS root Pyroscope origins, safe tags, and the absence of conflicting or per-pod profile tags;
- telemetry environment and Pulumi downward API wiring;
- Alloy egress ports 4318 and 4040;
- generated help and schemas for all nine `grafana_query` actions, both flat `grafana_render` actions, and the sole `grafana_exec` action;
- exact PromQL, TraceQL, and Profiles routes, methods, parameters, and bodies;
- action defaults, range limits, PromQL point limits, and strict flamegraph result normalization;
- shared four-request capacity, 30-second timeout, redirect denial, permit release, and error mapping;
- the 8192-byte encoded URL cap and 4 MiB decoded response cap;
- fixed dashboard inventory, rendering, alerting, mode, destination, and outcome telemetry values; and
- exclusion of dashboard and panel identifiers, render controls and image metadata, matchers, comments, alert data, URLs, credentials, query data, and upstream bodies from telemetry.

## Post-Deployment Evidence

After an authorized deployment, record evidence for each row. Use timestamps, non-secret query text, and redacted screenshots or exported results.

| Area | Required evidence |
| --- | --- |
| JSON logs | A startup event and one request completion parse as single JSON objects with approved fields only. |
| Signal filtering | Under `RUST_LOG=homelab_mcp=info`, Tempo contains `mcp.server.request` with `grafana.query` as its child for one authorized request. Under a permissive value, JSON logs still expose only the two approved target trees. Exporter and profiler dependency diagnostics remain absent. |
| Trace correlation | One request log has `trace_id` and `span_id`; Tempo contains that trace and matching span. |
| W3C propagation | A request with a known `traceparent` joins the caller trace. |
| Trace resources | Tempo shows service, namespace, environment, Kubernetes namespace, pod name, `k8s.pod.uid`, and matching `service.instance.id`. |
| HTTP metrics | Request count, active requests, and duration appear with bounded routes and outcomes. A cancelled request restores the active count and has no response status. `/health` and `/ready` produce no request telemetry. |
| MCP metrics | Generic `mcp.server.request.count`, `.duration`, and `.in_flight` appear with protocol request attributes and bounded outcomes. |
| Grafana metrics | Upstream requests, duration, and in-flight values appear with fixed datasource UIDs. |
| Profiles | A 100 Hz CPU profile appears for `homelab-mcp` with only approved stable tags. |
| LogQL | Authenticated instant and range calls return normalized data through UID `loki`. |
| PromQL | Authenticated instant and range calls return normalized data through UID `mimir`. |
| TraceQL | An authenticated search returns bounded traces through UID `tempo`. |
| Profiles action | An authenticated merge returns one top-level flamegraph with arrays `names` and `levels` plus string fields `total` and `maxSelf`. Other 200 JSON objects return `invalid_response`. |
| Alert rules | An authenticated list returns bounded normalized summaries without internal URLs, raw wrappers, or query models. |
| Alert instances | An authenticated matcher-filtered list returns bounded normalized current alerts without internal URLs or raw wrappers. |
| Dashboard inventory | Authenticated list and get calls return only bounded dashboard, variable, and flattened panel inventory. |
| Rendering | With Grafana 13.1.1 and separately deployed renderer 5.7.1, authenticated dashboard and panel calls return validated PNG image blocks to a Kuri `6eebdb0+` consumer and an image-capable selected model. This is required future evidence, not a live-validation claim. |
| Silence creation | An explicitly approved call creates one bounded silence and returns only its ID and interval. Confirm no automatic retry and inspect current silences after a controlled uncertain outcome. |
| Failure safety | A controlled invalid query returns a stable safe error without query data, URLs, or credentials. |
| Export outage | A controlled non-production outage leaves HTTP handling available. Missing or stale data appears in the affected backend, with evidence from Alloy or backend observability and no local dependency diagnostics. |
| Shutdown | Pyroscope stop and shutdown run on a dedicated thread. Main waits at most ten seconds, detaches a timed-out worker, then applies sequential five-second meter and tracer limits. Application logs contain only sanitized shutdown status. |

Use the [operations queries](../operations/observability.md#validation) for backend checks. Follow each action's specification under [Grafana tools](../grafana-query/README.md).

Do not record credentials, authorization headers, Secret values, full environment output, dashboard or panel UIDs, ranges, timezones, dimensions, variables, image bytes or digests, matchers, silence comments, alert data, internal URLs, or upstream bodies as evidence.
