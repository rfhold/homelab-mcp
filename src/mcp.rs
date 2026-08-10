#![allow(clippy::useless_vec)]

use std::{future::Future, sync::Arc};

use axum::Router;
use mcp::{
    McpProtectedResourceMetadata, McpToolResult, OAuthAuthorizationServer,
    server::{
        ServerContext, ServerError, ServerResult, StreamableHttpAuthorization,
        StreamableHttpOptions, streamable_http_router_with_options,
    },
};
use serde_json::json;

use crate::{
    config::OAuthConfig,
    integrations::grafana::{
        Error as GrafanaError,
        actions::{
            AlertInstancesInput, AlertRulesInput, CreateSilenceCommand, CreateSilenceInput,
            LogqlInput, ProfilesInput, PromqlInput, TraceqlInput,
        },
    },
    services::Services,
};

#[cfg(test)]
const QUERY_TOOL_NAME: &str = "grafana_query";
#[cfg(test)]
const EXEC_TOOL_NAME: &str = "grafana_exec";

#[derive(Clone)]
pub struct HomelabMcp {
    services: Arc<Services>,
}

pub fn router(
    config: &OAuthConfig,
    services: Arc<Services>,
    oauth: &OAuthAuthorizationServer,
) -> Result<Router, String> {
    let handler = Arc::new(HomelabMcp { services });
    let required_scope = config.required_scope.clone();
    let metadata =
        McpProtectedResourceMetadata::new(config.resource.clone(), [config.issuer.clone()])
            .with_scopes([required_scope.clone()])
            .with_resource_name("Homelab MCP");
    let hosted = oauth.clone();
    let authorization = StreamableHttpAuthorization::hosted(metadata, move |token, context| {
        hosted.authorize_token(token, context)
    })
    .map_err(|_| "invalid MCP authorization configuration".to_owned())?
    .with_required_scopes([required_scope]);
    let options = StreamableHttpOptions::default()
        .without_root_protected_resource_metadata()
        .with_authorization(authorization);
    Ok(streamable_http_router_with_options(handler, options))
}

#[mcp::progressive_server(
    name = "homelab-mcp",
    version = "0.1.0",
    description = "Authenticated homelab observability tools.",
    tool(
        name = "grafana_query",
        description = "Execute bounded, read-only Grafana queries.",
        annotations = json!({
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": true
        })
    ),
    tool(
        name = "grafana_exec",
        description = "Perform operationally consequential Grafana writes.",
        annotations = json!({
            "readOnlyHint": false,
            "destructiveHint": false,
            "idempotentHint": false,
            "openWorldHint": true
        })
    )
)]
impl HomelabMcp {
    /// Execute a LogQL instant or range query through Grafana.
    ///
    /// Use instant mode without start/end, or range mode with both endpoints.
    /// Log streams are normalized and limited to the requested number of lines.
    #[action(tool = "grafana_query", name = "logql")]
    async fn logql(
        &self,
        input: LogqlInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tool_error("LogQL", GrafanaError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.grafana.execute(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        match result {
            Ok(output) => Ok(query_result(output)),
            Err(error) => Ok(tool_error("LogQL", error)),
        }
    }

    /// Execute a PromQL instant or range query through Grafana.
    ///
    /// Range mode requires start, end, and a positive Prometheus duration step.
    /// Ranges are limited to 24 hours and 11,000 points.
    #[action(tool = "grafana_query", name = "promql")]
    async fn promql(
        &self,
        input: PromqlInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tool_error("PromQL", GrafanaError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.grafana.execute_promql(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        match result {
            Ok(output) => Ok(query_result(output)),
            Err(error) => Ok(tool_error("PromQL", error)),
        }
    }

    /// Search traces with TraceQL through Grafana.
    ///
    /// Optional start and end timestamps must appear together. Searches are
    /// limited to 24 hours and at most 100 returned traces.
    #[action(tool = "grafana_query", name = "traceql")]
    async fn traceql(
        &self,
        input: TraceqlInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tool_error("TraceQL", GrafanaError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.grafana.execute_traceql(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        match result {
            Ok(output) => Ok(query_result(output)),
            Err(error) => Ok(tool_error("TraceQL", error)),
        }
    }

    /// Merge Pyroscope stacktraces through Grafana.
    ///
    /// Start and end are required RFC3339 timestamps. Profile ranges are
    /// limited to one hour and at most 1,000 flame graph nodes.
    #[action(tool = "grafana_query", name = "profiles")]
    async fn profiles(
        &self,
        input: ProfilesInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tool_error("profile", GrafanaError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.grafana.execute_profiles(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        match result {
            Ok(output) => Ok(query_result(output)),
            Err(error) => Ok(tool_error("profile", error)),
        }
    }

    /// List bounded Grafana alert-rule summaries.
    #[action(tool = "grafana_query", name = "alert_rules")]
    async fn alert_rules(
        &self,
        input: AlertRulesInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tool_error("alert rule", GrafanaError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.grafana.alert_rules(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        match result {
            Ok(output) => Ok(query_result(output)),
            Err(error) => Ok(tool_error("alert rule", error)),
        }
    }

    /// List bounded current Grafana alert instances, optionally filtered by labels.
    #[action(tool = "grafana_query", name = "alert_instances")]
    async fn alert_instances(
        &self,
        input: AlertInstancesInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => {
                return Ok(tool_error("alert instance", GrafanaError::InvalidArguments));
            }
        };
        let result = tokio::select! {
            result = self.services.grafana.alert_instances(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        match result {
            Ok(output) => Ok(query_result(output)),
            Err(error) => Ok(tool_error("alert instance", error)),
        }
    }

    /// Create a bounded Grafana silence that suppresses matching alert notifications.
    #[action(tool = "grafana_exec", name = "create_silence")]
    async fn create_silence(
        &self,
        input: CreateSilenceInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let command = match input.validate() {
            Ok(command) => command,
            Err(_) => return Ok(tool_error("silence", GrafanaError::InvalidArguments)),
        };
        Ok(self
            .dispatch_create_silence(&command, context.cancelled())
            .await)
    }

    async fn dispatch_create_silence(
        &self,
        command: &CreateSilenceCommand,
        cancellation: impl Future<Output = ()>,
    ) -> McpToolResult {
        let result = tokio::select! {
            result = self.services.grafana.create_silence(command) => result,
            () = cancellation => return tool_error("silence", GrafanaError::MutationOutcomeUnknown),
        };
        match result {
            Ok(output) => silence_result(output),
            Err(error) => tool_error("silence", error),
        }
    }
}

fn query_result(output: serde_json::Value) -> McpToolResult {
    let mode = output["mode"].as_str().unwrap_or("query");
    let result_type = output["result_type"].as_str().unwrap_or("unknown");
    let count = output["result"].as_array().map_or(1, Vec::len);
    McpToolResult::new(json!({
        "content": [{"type":"text","text":format!("{mode} {result_type} result with {count} item(s).")}],
        "structuredContent": output
    }))
}

fn silence_result(output: serde_json::Value) -> McpToolResult {
    let silence_id = output["silence_id"].as_str().unwrap_or("unknown");
    McpToolResult::new(json!({
        "content": [{"type":"text","text":format!("Created Grafana silence {silence_id}.")}],
        "structuredContent": output
    }))
}

fn tool_error(query_name: &str, error: GrafanaError) -> McpToolResult {
    error.into_tool_error(query_name).into_mcp_result()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use crate::integrations::grafana::GrafanaClient;
    use axum::{
        Json, Router,
        routing::{get, post},
    };
    use mcp::{
        McpPrincipalId,
        protocol::MCP_PROTOCOL_VERSION,
        server::{
            McpHostedTokenValidation, McpTokenAuthorization, StreamableHttpAuthorization,
            StreamableHttpOptions, streamable_http_router,
        },
    };
    use opentelemetry::trace::{SpanId, TracerProvider as _};
    use opentelemetry_sdk::{
        error::OTelSdkResult,
        trace::{SdkTracerProvider, SpanData, SpanExporter},
    };
    use reqwest::{Client, StatusCode};
    use serde_json::Value;
    use tokio::{net::TcpListener, sync::Notify, task::JoinHandle};
    use tracing_subscriber::{Layer as _, layer::SubscriberExt as _};

    use super::*;

    async fn serve(router: Router) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (origin, task)
    }

    async fn test_handler() -> (Arc<HomelabMcp>, JoinHandle<()>) {
        let grafana = Router::new()
            .route(
                "/api/datasources/proxy/uid/loki/loki/api/v1/query",
                get(|| async {
                    Json(json!({
                        "status":"success",
                        "data":{"resultType":"vector","result":[{"metric":{"job":"test"},"value":[1786276800,"2"]}]}
                    }))
                }),
            )
            .route(
                "/api/v1/provisioning/alert-rules",
                get(|| async {
                    Json(json!([{
                        "uid":"rule-1", "title":"API errors", "folderUID":"folder-1",
                        "ruleGroup":"api", "condition":"C", "noDataState":"NoData",
                        "execErrState":"Error", "for":"5m", "isPaused":false,
                        "labels":{"severity":"critical"}, "annotations":{"summary":"API is failing"}
                    }]))
                }),
            )
            .route(
                "/api/alertmanager/grafana/api/v2/alerts",
                get(|| async {
                    Json(json!([{
                        "fingerprint":"abc123", "startsAt":"2026-08-10T12:00:00Z",
                        "endsAt":"2026-08-10T13:00:00Z", "updatedAt":"2026-08-10T12:01:00Z",
                        "status":{"state":"active","silencedBy":[],"inhibitedBy":[]},
                        "labels":{"alertname":"APIError"}, "annotations":{"summary":"API is failing"}
                    }]))
                }),
            )
            .route(
                "/api/alertmanager/grafana/api/v2/silences",
                post(|| async { Json(json!({"silenceID":"silence-123"})) }),
            );
        let (origin, task) = serve(grafana).await;
        let handler = Arc::new(HomelabMcp {
            services: Arc::new(Services::new(GrafanaClient::for_test(
                url::Url::parse(&format!("{origin}/")).unwrap(),
                std::time::Duration::from_secs(1),
            ))),
        });
        (handler, task)
    }

    fn request(method: &str, id: &str, params: Value) -> Value {
        let mut body = json!({
            "jsonrpc":"2.0", "id":id, "method":method, "params":params
        });
        body["params"]["_meta"] = json!({
            "io.modelcontextprotocol/protocolVersion": MCP_PROTOCOL_VERSION,
            "io.modelcontextprotocol/clientCapabilities": {},
            "io.modelcontextprotocol/clientInfo": {"name":"homelab-tests","version":"1"}
        });
        body
    }

    async fn post_mcp(endpoint: &str, body: Value) -> (StatusCode, Value) {
        let method = body["method"].as_str().unwrap();
        let mut request = Client::new()
            .post(endpoint)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
            .header("mcp-method", method);
        if let Some(name) = body["params"]["name"].as_str() {
            request = request.header("mcp-name", name);
        }
        let response = request.json(&body).send().await.unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        let payload = text
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix("data: "))
            .unwrap_or(&text);
        (status, serde_json::from_str(payload).unwrap())
    }

    #[derive(Clone, Debug, Default)]
    struct TestSpanExporter(Arc<Mutex<Vec<SpanData>>>);

    impl SpanExporter for TestSpanExporter {
        async fn export(&self, mut batch: Vec<SpanData>) -> OTelSdkResult {
            self.0.lock().unwrap().append(&mut batch);
            Ok(())
        }
    }

    #[derive(Clone)]
    struct SpanTargetCapture(Arc<Mutex<Vec<(&'static str, &'static str)>>>);

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for SpanTargetCapture {
        fn on_new_span(
            &self,
            attributes: &tracing::span::Attributes<'_>,
            _id: &tracing::span::Id,
            _context: tracing_subscriber::layer::Context<'_, S>,
        ) {
            let metadata = attributes.metadata();
            self.0
                .lock()
                .unwrap()
                .push((metadata.name(), metadata.target()));
        }
    }

    fn capture_authorized_call(
        runtime: &tokio::runtime::Runtime,
    ) -> (Vec<SpanData>, Vec<(&'static str, &'static str)>) {
        let exporter = TestSpanExporter::default();
        let targets = Arc::new(Mutex::new(Vec::new()));
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let ignored_json_targets = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry()
            .with(SpanTargetCapture(targets.clone()))
            .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("homelab-mcp-test")))
            .with(
                SpanTargetCapture(ignored_json_targets)
                    .with_filter(crate::observability::test_json_filter()),
            )
            .with(crate::observability::test_trace_filter());

        tracing::subscriber::with_default(subscriber, || {
            runtime.block_on(async {
                let (handler, grafana_task) = test_handler().await;
                let metadata = McpProtectedResourceMetadata::new(
                    "https://mcp.example.test/mcp",
                    ["https://mcp.example.test/oauth"],
                )
                .with_scopes(["mcp:use"]);
                let authorization = StreamableHttpAuthorization::hosted(metadata, |_, _| {
                    Box::pin(async {
                        McpHostedTokenValidation::Authorized(McpTokenAuthorization {
                            principal_id: McpPrincipalId::new("telemetry-test").unwrap(),
                            expires_at: None,
                            revocation: None,
                        })
                    })
                })
                .unwrap();
                let authorization = authorization.with_required_scopes(["mcp:use"]);
                let router = streamable_http_router_with_options(
                    handler,
                    StreamableHttpOptions::default()
                        .without_root_protected_resource_metadata()
                        .with_authorization(authorization),
                );
                let (origin, mcp_task) = serve(router).await;
                let endpoint = format!("{origin}/mcp");
                let body = request(
                    "tools/call",
                    "telemetry-call",
                    json!({
                        "name": QUERY_TOOL_NAME,
                        "arguments": {
                            "action": "logql",
                            "input": {"query": "{job=\"telemetry-test\"}"}
                        }
                    }),
                );
                let response = Client::new()
                    .post(endpoint)
                    .header("accept", "application/json, text/event-stream")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
                    .header("mcp-method", "tools/call")
                    .header("mcp-name", QUERY_TOOL_NAME)
                    .json(&body)
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                let payload = response.text().await.unwrap();
                assert!(payload.contains("structuredContent"));
                assert!(!payload.contains("test-token"));
                grafana_task.abort();
                mcp_task.abort();
            });
        });

        provider.force_flush().unwrap();
        let spans = exporter.0.lock().unwrap().clone();
        let targets = targets.lock().unwrap().clone();
        (spans, targets)
    }

    #[test]
    fn host_filters_export_kuri_request_as_grafana_parent() {
        const CHILD_MARKER: &str = "HOMELAB_MCP_TELEMETRY_TEST_CHILD";
        if std::env::var_os(CHILD_MARKER).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "mcp::tests::host_filters_export_kuri_request_as_grafana_parent",
                    "--nocapture",
                ])
                .env(CHILD_MARKER, "1")
                .env("RUST_LOG", "homelab_mcp=info")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (spans, targets) = capture_authorized_call(&runtime);
        let exported = spans
            .iter()
            .map(|span| (span.name.as_ref(), span.instrumentation_scope.name()))
            .collect::<Vec<_>>();
        let server = spans
            .iter()
            .find(|span| span.name == "mcp.server.request")
            .unwrap_or_else(|| panic!("missing Kuri server span; exported {exported:?}"));
        let grafana = spans
            .iter()
            .find(|span| span.name == "grafana.query")
            .unwrap();

        assert!(targets.contains(&("mcp.server.request", "mcp::server")));
        assert!(targets.contains(&("grafana.query", "homelab_mcp::grafana")));
        assert_eq!(server.parent_span_id, SpanId::INVALID);
        assert_eq!(grafana.parent_span_id, server.span_context.span_id());
        assert_eq!(
            grafana.span_context.trace_id(),
            server.span_context.trace_id()
        );
    }

    #[tokio::test]
    async fn discovery_list_help_filter_and_call_follow_progressive_contract() {
        let (handler, grafana_task) = test_handler().await;
        let (origin, mcp_task) = serve(streamable_http_router(handler)).await;
        let endpoint = format!("{origin}/mcp");

        let (_, discover) =
            post_mcp(&endpoint, request("server/discover", "discover", json!({}))).await;
        assert_eq!(
            discover["result"]["supportedVersions"],
            json!(["2026-07-28"])
        );
        assert_eq!(
            discover["result"]["capabilities"],
            json!({"tools":{"listChanged":false}})
        );
        assert_eq!(discover["result"]["cacheScope"], "private");
        assert_eq!(discover["result"]["ttlMs"], 0);

        let (_, listed) = post_mcp(&endpoint, request("tools/list", "list", json!({}))).await;
        let tools = listed["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        let query_tool = tools
            .iter()
            .find(|tool| tool["name"] == QUERY_TOOL_NAME)
            .unwrap();
        let exec_tool = tools
            .iter()
            .find(|tool| tool["name"] == EXEC_TOOL_NAME)
            .unwrap();
        assert_eq!(
            query_tool["annotations"],
            json!({
                "readOnlyHint":true, "destructiveHint":false,
                "idempotentHint":true, "openWorldHint":true
            })
        );
        assert_eq!(
            exec_tool["annotations"],
            json!({
                "readOnlyHint":false, "destructiveHint":false,
                "idempotentHint":false, "openWorldHint":true
            })
        );
        assert!(
            exec_tool["description"]
                .as_str()
                .unwrap()
                .contains("operationally consequential")
        );
        assert_eq!(query_tool["inputSchema"]["additionalProperties"], false);
        assert_eq!(exec_tool["inputSchema"]["additionalProperties"], false);

        let (_, query_help) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "query-help",
                json!({"name":QUERY_TOOL_NAME,"arguments":{"action":"help","filter":".actions"}}),
            ),
        )
        .await;
        let query_actions = query_help["result"]["structuredContent"]["result"]
            .as_array()
            .unwrap();
        assert_eq!(
            query_actions
                .iter()
                .map(|action| action["action"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec![
                "logql",
                "promql",
                "traceql",
                "profiles",
                "alert_rules",
                "alert_instances"
            ]
        );
        for (action, required, optional) in [
            (
                "promql",
                vec!["query"],
                vec!["start", "end", "step", "time"],
            ),
            ("traceql", vec!["query"], vec!["start", "end", "limit"]),
            (
                "profiles",
                vec!["selector", "start", "end"],
                vec!["profile_type", "max_nodes"],
            ),
            ("alert_rules", vec![], vec!["limit"]),
            ("alert_instances", vec![], vec!["matchers", "limit"]),
        ] {
            let schema = &query_actions
                .iter()
                .find(|candidate| candidate["action"] == action)
                .unwrap()["input_schema"];
            assert_eq!(schema["additionalProperties"], false);
            let properties = schema["properties"].as_object().unwrap();
            for field in required.iter().chain(optional.iter()) {
                assert!(properties.contains_key(*field), "{action} missing {field}");
            }
            if required.is_empty() {
                assert_eq!(schema["required"], Value::Null);
            } else {
                assert_eq!(schema["required"], json!(required));
            }
        }

        let (_, exec_help) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "exec-help",
                json!({"name":EXEC_TOOL_NAME,"arguments":{"action":"help","filter":".actions"}}),
            ),
        )
        .await;
        let exec_actions = exec_help["result"]["structuredContent"]["result"]
            .as_array()
            .unwrap();
        assert_eq!(exec_actions.len(), 1);
        assert_eq!(exec_actions[0]["action"], "create_silence");
        let silence_schema = &exec_actions[0]["input_schema"];
        assert_eq!(silence_schema["additionalProperties"], false);
        assert_eq!(
            silence_schema["required"],
            json!(["matchers", "duration_seconds", "comment"])
        );

        for (action, result_type, expected_field) in [
            ("alert_rules", "alert_rules", ("title", "API errors")),
            (
                "alert_instances",
                "alert_instances",
                ("fingerprint", "abc123"),
            ),
        ] {
            let (_, call) = post_mcp(
                &endpoint,
                request(
                    "tools/call",
                    action,
                    json!({"name":QUERY_TOOL_NAME,"arguments":{"action":action,"input":{}}}),
                ),
            )
            .await;
            assert_eq!(call["result"]["isError"], Value::Null, "{call}");
            assert_eq!(call["result"]["structuredContent"]["mode"], "list");
            assert_eq!(
                call["result"]["structuredContent"]["result_type"],
                result_type
            );
            assert_eq!(
                call["result"]["structuredContent"]["result"][0][expected_field.0],
                expected_field.1
            );
        }

        let (_, silence) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "create-silence",
                json!({
                    "name":EXEC_TOOL_NAME,
                    "arguments":{
                        "action":"create_silence",
                        "input":{
                            "matchers":[{"name":"alertname","operator":"=","value":"APIError"}],
                            "duration_seconds":3600,
                            "comment":"maintenance"
                        }
                    }
                }),
            ),
        )
        .await;
        assert_eq!(silence["result"]["isError"], Value::Null, "{silence}");
        let structured = &silence["result"]["structuredContent"];
        assert_eq!(structured["silence_id"], "silence-123");
        assert!(structured["starts_at"].is_string());
        assert!(structured["ends_at"].is_string());
        assert_eq!(structured.as_object().unwrap().len(), 3);
        assert_eq!(
            silence["result"]["content"][0]["text"],
            "Created Grafana silence silence-123."
        );

        let (_, filtered) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "filtered-alerts",
                json!({
                    "name":QUERY_TOOL_NAME,
                    "arguments":{"action":"alert_instances","input":{},"filter":".result[]"}
                }),
            ),
        )
        .await;
        assert_eq!(
            filtered["result"]["structuredContent"]["fingerprint"],
            "abc123"
        );

        grafana_task.abort();
        mcp_task.abort();
    }

    #[tokio::test]
    async fn malformed_shapes_actions_filters_and_semantic_failures_keep_error_boundary() {
        let (handler, grafana_task) = test_handler().await;
        let (origin, mcp_task) = serve(streamable_http_router(handler)).await;
        let endpoint = format!("{origin}/mcp");
        for (tool, arguments) in [
            (QUERY_TOOL_NAME, json!({"action":"unknown"})),
            (QUERY_TOOL_NAME, json!({"action":"create_silence"})),
            (EXEC_TOOL_NAME, json!({"action":"alert_rules"})),
            (
                QUERY_TOOL_NAME,
                json!({"action":"alert_rules","input":{"limit":1,"extra":true}}),
            ),
            (
                EXEC_TOOL_NAME,
                json!({
                    "action":"create_silence",
                    "input":{
                        "matchers":[], "duration_seconds":1, "comment":"x", "extra":true
                    }
                }),
            ),
            (QUERY_TOOL_NAME, json!({"action":"help","extra":true})),
            (QUERY_TOOL_NAME, json!({"action":"help","filter":".["})),
        ] {
            let (_, response) = post_mcp(
                &endpoint,
                request(
                    "tools/call",
                    "invalid",
                    json!({"name":tool,"arguments":arguments}),
                ),
            )
            .await;
            assert_eq!(response["error"]["code"], -32602, "{response}");
        }

        let (_, semantic) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "semantic",
                json!({"name":QUERY_TOOL_NAME,"arguments":{"action":"logql","input":{"query":" "}}}),
            ),
        )
        .await;
        assert_eq!(semantic["result"]["isError"], true);
        assert_eq!(
            semantic["result"]["structuredContent"]["error"],
            json!({"code":"invalid_arguments","message":"The LogQL arguments are invalid.","retryable":false})
        );
        assert!(!semantic.to_string().contains("grafana-secret"));

        let (_, mutation_semantic) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "mutation-semantic",
                json!({
                    "name":EXEC_TOOL_NAME,
                    "arguments":{
                        "action":"create_silence",
                        "input":{"matchers":[],"duration_seconds":0,"comment":" "}
                    }
                }),
            ),
        )
        .await;
        assert_eq!(mutation_semantic["result"]["isError"], true);
        assert_eq!(
            mutation_semantic["result"]["structuredContent"]["error"],
            json!({"code":"invalid_arguments","message":"The silence arguments are invalid.","retryable":false})
        );

        let (_, unknown_tool) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "unknown-tool",
                json!({"name":"other","arguments":{"action":"help"}}),
            ),
        )
        .await;
        // The 2026-07-28 transport rejects an unknown dynamic Mcp-Name before handler dispatch.
        assert_eq!(unknown_tool["error"]["code"], -32020);
        assert!(unknown_tool.get("result").is_none());

        grafana_task.abort();
        mcp_task.abort();
    }

    #[tokio::test]
    async fn uncertain_mutation_failure_is_exact_safe_and_non_retryable() {
        let grafana = Router::new().route(
            "/api/alertmanager/grafana/api/v2/silences",
            post(|| async {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "unsafe upstream mutation detail",
                )
            }),
        );
        let (grafana_origin, grafana_task) = serve(grafana).await;
        let handler = Arc::new(HomelabMcp {
            services: Arc::new(Services::new(GrafanaClient::for_test(
                url::Url::parse(&format!("{grafana_origin}/")).unwrap(),
                std::time::Duration::from_secs(1),
            ))),
        });
        let (origin, mcp_task) = serve(streamable_http_router(handler)).await;
        let (_, response) = post_mcp(
            &format!("{origin}/mcp"),
            request(
                "tools/call",
                "uncertain-mutation",
                json!({
                    "name":EXEC_TOOL_NAME,
                    "arguments":{
                        "action":"create_silence",
                        "input":{
                            "matchers":[{"name":"alertname","operator":"=","value":"APIError"}],
                            "duration_seconds":3600,
                            "comment":"maintenance"
                        }
                    }
                }),
            ),
        )
        .await;

        let message = "The Grafana mutation did not complete cleanly; its outcome may be uncertain. Check existing silences before retrying.";
        assert_eq!(response["result"]["isError"], true);
        assert_eq!(
            response["result"]["structuredContent"]["error"],
            json!({"code":"mutation_outcome_unknown","message":message,"retryable":false})
        );
        assert_eq!(response["result"]["content"][0]["text"], message);
        assert!(
            !response
                .to_string()
                .contains("unsafe upstream mutation detail")
        );

        grafana_task.abort();
        mcp_task.abort();
    }

    #[tokio::test]
    async fn cancellation_after_silence_post_is_exact_safe_and_non_retryable() {
        let post_received = Arc::new(Notify::new());
        let post_probe = Arc::clone(&post_received);
        let grafana = Router::new().route(
            "/api/alertmanager/grafana/api/v2/silences",
            post(move || {
                let post_probe = Arc::clone(&post_probe);
                async move {
                    post_probe.notify_one();
                    std::future::pending::<Json<Value>>().await
                }
            }),
        );
        let (grafana_origin, grafana_task) = serve(grafana).await;
        let handler = Arc::new(HomelabMcp {
            services: Arc::new(Services::new(GrafanaClient::for_test(
                url::Url::parse(&format!("{grafana_origin}/")).unwrap(),
                std::time::Duration::from_secs(1),
            ))),
        });
        let cancellation = Arc::new(Notify::new());
        let cancellation_signal = Arc::clone(&cancellation);
        let dispatch = tokio::spawn(async move {
            let command = CreateSilenceInput {
                matchers: vec![crate::integrations::grafana::actions::LabelMatcher {
                    name: "alertname".to_owned(),
                    operator: crate::integrations::grafana::actions::MatcherOperator::Equal,
                    value: "APIError".to_owned(),
                }],
                duration_seconds: 3600,
                comment: "maintenance".to_owned(),
            }
            .validate()
            .unwrap();
            handler
                .dispatch_create_silence(&command, cancellation_signal.notified())
                .await
        });

        post_received.notified().await;
        cancellation.notify_one();
        let response = dispatch.await.unwrap().raw;

        let message = "The Grafana mutation did not complete cleanly; its outcome may be uncertain. Check existing silences before retrying.";
        assert_eq!(response["isError"], true);
        assert_eq!(
            response["structuredContent"]["error"],
            json!({"code":"mutation_outcome_unknown","message":message,"retryable":false})
        );
        assert_eq!(response["content"][0]["text"], message);
        assert!(!response.to_string().contains("request cancelled"));

        grafana_task.abort();
    }

    #[test]
    fn semantic_errors_are_exact_and_safe() {
        let cases = [
            (
                GrafanaError::InvalidArguments,
                "invalid_arguments",
                "The LogQL arguments are invalid.",
                false,
            ),
            (
                GrafanaError::CapacityExhausted,
                "capacity_exhausted",
                "Grafana request capacity is currently exhausted.",
                true,
            ),
            (
                GrafanaError::Timeout,
                "timeout",
                "The Grafana query timed out.",
                true,
            ),
            (
                GrafanaError::Unauthorized,
                "grafana_unauthorized",
                "Grafana rejected the service credentials.",
                false,
            ),
            (
                GrafanaError::QueryRejected,
                "query_rejected",
                "Grafana rejected the LogQL query.",
                false,
            ),
            (
                GrafanaError::MutationRejected,
                "mutation_rejected",
                "Grafana rejected the requested mutation.",
                false,
            ),
            (
                GrafanaError::MutationOutcomeUnknown,
                "mutation_outcome_unknown",
                "The Grafana mutation did not complete cleanly; its outcome may be uncertain. Check existing silences before retrying.",
                false,
            ),
            (
                GrafanaError::UpstreamUnavailable,
                "upstream_unavailable",
                "Grafana is currently unavailable.",
                true,
            ),
            (
                GrafanaError::InvalidResponse,
                "invalid_response",
                "Grafana returned an invalid response.",
                false,
            ),
        ];
        for (error, code, message, retryable) in cases {
            let result = tool_error("LogQL", error).raw;
            assert_eq!(result["isError"], true);
            assert_eq!(
                result["structuredContent"]["error"],
                json!({"code":code,"message":message,"retryable":retryable})
            );
            assert_eq!(result["content"][0]["text"], message);
            assert!(!result.to_string().contains("secret"));
        }
    }

    #[tokio::test]
    async fn generic_hosted_authorization_supplies_challenge_and_origin_denial() {
        let (handler, grafana_task) = test_handler().await;
        let metadata = McpProtectedResourceMetadata::new(
            "http://127.0.0.1/mcp",
            ["https://auth.example.com/oauth"],
        )
        .with_scopes(["homelab:use"]);
        let authorization = StreamableHttpAuthorization::hosted(metadata, |_, _| {
            Box::pin(async { McpHostedTokenValidation::Unavailable })
        })
        .unwrap()
        .with_required_scopes(["homelab:use"]);
        let router = streamable_http_router_with_options(
            handler,
            StreamableHttpOptions::default().with_authorization(authorization),
        );
        let (origin, mcp_task) = serve(router).await;
        let endpoint = format!("{origin}/mcp");
        let body = request("server/discover", "auth", json!({}));
        let client = Client::new();
        let missing = client
            .post(&endpoint)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
            .header("mcp-method", "server/discover")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
        let challenge = missing.headers()["www-authenticate"].to_str().unwrap();
        assert!(challenge.starts_with("Bearer "));
        assert!(challenge.contains("homelab:use"));

        let unavailable = client
            .post(&endpoint)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
            .header("mcp-method", "server/discover")
            .header("authorization", "Bearer local-token")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(unavailable.status(), StatusCode::SERVICE_UNAVAILABLE);

        let forbidden_origin = client
            .post(&endpoint)
            .header("origin", "https://attacker.example")
            .send()
            .await
            .unwrap();
        assert_eq!(forbidden_origin.status(), StatusCode::FORBIDDEN);

        grafana_task.abort();
        mcp_task.abort();
    }
}
