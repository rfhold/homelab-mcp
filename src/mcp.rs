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
            ListSilencesInput, LogqlInput, ProfilesInput, PromqlInput, TraceqlInput,
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
        }),
        namespace(name = "logql", description = "Query Loki logs with LogQL."),
        namespace(name = "promql", description = "Query Mimir metrics with PromQL."),
        namespace(name = "traceql", description = "Search Tempo traces with TraceQL."),
        namespace(name = "profile", description = "Inspect Pyroscope profiles."),
        namespace(name = "alert-rule", description = "Inspect Grafana alert rules."),
        namespace(name = "alert-instance", description = "Inspect current Grafana alert instances."),
        namespace(name = "silence", description = "Inspect Grafana alert silences.")
    ),
    tool(
        name = "grafana_exec",
        description = "Perform operationally consequential Grafana writes.",
        annotations = json!({
            "readOnlyHint": false,
            "destructiveHint": false,
            "idempotentHint": false,
            "openWorldHint": true
        }),
        namespace(name = "silence", description = "Manage Grafana alert silences.")
    )
)]
impl HomelabMcp {
    /// Execute a LogQL instant or range query through Grafana.
    ///
    /// Use instant mode without start/end, or range mode with both endpoints.
    /// Log streams are normalized and limited to the requested number of lines.
    #[action(tool = "grafana_query", name = "logql.query")]
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
    #[action(tool = "grafana_query", name = "promql.query")]
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
    #[action(tool = "grafana_query", name = "traceql.search")]
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
    #[action(tool = "grafana_query", name = "profile.merge")]
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
    #[action(tool = "grafana_query", name = "alert-rule.list")]
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
    #[action(tool = "grafana_query", name = "alert-instance.list")]
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

    /// List bounded Grafana silences, optionally filtered by state.
    #[action(tool = "grafana_query", name = "silence.list")]
    async fn list_silences(
        &self,
        input: ListSilencesInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tool_error("silence", GrafanaError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.grafana.list_silences(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        match result {
            Ok(output) => Ok(query_result(output)),
            Err(error) => Ok(tool_error("silence", error)),
        }
    }

    /// Create a bounded Grafana silence that suppresses matching alert notifications.
    #[action(tool = "grafana_exec", name = "silence.create")]
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
        extract::State,
        http::HeaderMap,
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
    use opentelemetry::trace::{SpanId, SpanKind, Status, TracerProvider as _};
    use opentelemetry_sdk::{
        error::OTelSdkResult,
        trace::{SdkTracerProvider, SpanData, SpanExporter},
    };
    use reqwest::{Client, StatusCode};
    use serde_json::Value;
    use tokio::{io::AsyncWriteExt as _, net::TcpListener, sync::Notify, task::JoinHandle};
    use tracing_subscriber::{Layer as _, layer::SubscriberExt as _};

    use super::*;

    type PropagatedRequests = Arc<Mutex<Vec<(String, String)>>>;

    async fn serve(router: Router) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (origin, task)
    }

    async fn test_handler() -> (Arc<HomelabMcp>, PropagatedRequests, JoinHandle<()>) {
        let propagated = Arc::new(Mutex::new(Vec::new()));
        let grafana = Router::new()
            .route(
                "/api/datasources/proxy/uid/loki/loki/api/v1/query",
                get(
                    |State(propagated): State<PropagatedRequests>,
                     headers: HeaderMap| async move {
                        record_propagated_context(&propagated, "GET", &headers);
                    Json(json!({
                        "status":"success",
                        "data":{"resultType":"vector","result":[{"metric":{"job":"test"},"value":[1786276800,"2"]}]}
                    }))
                    },
                ),
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
                get(|| async {
                    Json(json!([{
                        "id":"silence-active", "status":{"state":"active"},
                        "startsAt":"2026-08-10T12:00:00Z", "endsAt":"2026-08-10T13:00:00Z",
                        "createdBy":"homelab-mcp", "comment":"maintenance",
                        "matchers":[{"name":"alertname","value":"APIError","isRegex":false,"isEqual":true}]
                    }]))
                })
                .post(
                    |State(propagated): State<PropagatedRequests>,
                     headers: HeaderMap,
                     Json(_body): Json<Value>| async move {
                        record_propagated_context(&propagated, "POST", &headers);
                        Json(json!({"silenceID":"silence-123"}))
                    },
                ),
            )
            .with_state(Arc::clone(&propagated));
        let (origin, task) = serve(grafana).await;
        let handler = Arc::new(HomelabMcp {
            services: Arc::new(Services::new(GrafanaClient::for_test(
                url::Url::parse(&format!("{origin}/")).unwrap(),
                std::time::Duration::from_secs(1),
            ))),
        });
        (handler, propagated, task)
    }

    fn record_propagated_context(
        propagated: &Mutex<Vec<(String, String)>>,
        method: &str,
        headers: &HeaderMap,
    ) {
        propagated.lock().unwrap().push((
            method.to_owned(),
            headers
                .get("traceparent")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned(),
        ));
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

    struct TelemetryCapture {
        spans: Vec<SpanData>,
        targets: Vec<(&'static str, &'static str)>,
        propagated: Vec<(String, String)>,
    }

    fn capture_authorized_call(runtime: &tokio::runtime::Runtime) -> TelemetryCapture {
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

        let propagated = tracing::subscriber::with_default(subscriber, || {
            runtime.block_on(async {
                opentelemetry::global::set_text_map_propagator(
                    opentelemetry_sdk::propagation::TraceContextPropagator::new(),
                );
                let (handler, propagated, grafana_task) = test_handler().await;
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
                            "action": "logql.query",
                            "input": {"query": "{job=\"telemetry-test\"}"}
                        }
                    }),
                );
                let response = Client::new()
                    .post(&endpoint)
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

                let body = request(
                    "tools/call",
                    "telemetry-post",
                    json!({
                        "name": EXEC_TOOL_NAME,
                        "arguments": {
                            "action": "silence.create",
                            "input": {
                                "matchers": [{"name":"alertname","operator":"=","value":"SensitiveMatcher"}],
                                "duration_seconds": 3600,
                                "comment": "SensitiveComment"
                            }
                        }
                    }),
                );
                let response = Client::new()
                    .post(&endpoint)
                    .header("accept", "application/json, text/event-stream")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
                    .header("mcp-method", "tools/call")
                    .header("mcp-name", EXEC_TOOL_NAME)
                    .json(&body)
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                assert!(response.text().await.unwrap().contains("structuredContent"));

                let disconnect_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let disconnect_origin = url::Url::parse(&format!(
                    "http://{}/sensitive-path?secret=query",
                    disconnect_listener.local_addr().unwrap()
                ))
                .unwrap();
                let disconnect_task = tokio::spawn(async move {
                    let (connection, _) = disconnect_listener.accept().await.unwrap();
                    drop(connection);
                });
                let unavailable = GrafanaClient::for_test(
                    disconnect_origin,
                    std::time::Duration::from_secs(1),
                );
                assert!(
                    unavailable
                        .execute(
                            &crate::integrations::grafana::actions::LogqlInput {
                                query: "SensitiveRawErrorQuery".to_owned(),
                                start: None,
                                end: None,
                                time: None,
                                direction: None,
                                limit: None,
                            }
                            .validate()
                            .unwrap()
                        )
                        .await
                        .is_err()
                );
                disconnect_task.await.unwrap();

                let http_error = Router::new().route(
                    "/api/datasources/proxy/uid/loki/loki/api/v1/query",
                    get(|| async { StatusCode::SERVICE_UNAVAILABLE }),
                );
                let (origin, http_error_task) = serve(http_error).await;
                let unavailable = GrafanaClient::for_test(
                    url::Url::parse(&format!("{origin}/")).unwrap(),
                    std::time::Duration::from_secs(1),
                );
                assert!(
                    unavailable
                        .execute(
                            &crate::integrations::grafana::actions::LogqlInput {
                                query: "SensitiveHttpErrorQuery".to_owned(),
                                start: None,
                                end: None,
                                time: None,
                                direction: None,
                                limit: None,
                            }
                            .validate()
                            .unwrap()
                        )
                        .await
                        .is_err()
                );
                http_error_task.abort();

                let slow = Router::new().route(
                    "/api/datasources/proxy/uid/loki/loki/api/v1/query",
                    get(|| async {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        Json(json!({}))
                    }),
                );
                let (origin, slow_task) = serve(slow).await;
                let timeout = GrafanaClient::for_test(
                    url::Url::parse(&format!("{origin}/")).unwrap(),
                    std::time::Duration::from_millis(10),
                );
                assert_eq!(
                    timeout
                        .execute(
                            &crate::integrations::grafana::actions::LogqlInput {
                                query: "SensitiveTimeoutQuery".to_owned(),
                                start: None,
                                end: None,
                                time: None,
                                direction: None,
                                limit: None,
                            }
                            .validate()
                            .unwrap()
                        )
                        .await,
                    Err(crate::integrations::grafana::Error::Timeout)
                );
                slow_task.abort();

                let truncated_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let truncated_origin = url::Url::parse(&format!(
                    "http://{}/",
                    truncated_listener.local_addr().unwrap()
                ))
                .unwrap();
                let truncated_task = tokio::spawn(async move {
                    let (mut connection, _) = truncated_listener.accept().await.unwrap();
                    connection
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{",
                        )
                        .await
                        .unwrap();
                });
                let truncated = GrafanaClient::for_test(
                    truncated_origin,
                    std::time::Duration::from_secs(1),
                );
                assert_eq!(
                    truncated
                        .execute(
                            &crate::integrations::grafana::actions::LogqlInput {
                                query: "SensitiveTruncatedBodyQuery".to_owned(),
                                start: None,
                                end: None,
                                time: None,
                                direction: None,
                                limit: None,
                            }
                            .validate()
                            .unwrap()
                        )
                        .await,
                    Err(crate::integrations::grafana::Error::UpstreamUnavailable)
                );
                truncated_task.await.unwrap();

                let oversized_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let oversized_origin = url::Url::parse(&format!(
                    "http://{}/",
                    oversized_listener.local_addr().unwrap()
                ))
                .unwrap();
                let oversized_task = tokio::spawn(async move {
                    let (mut connection, _) = oversized_listener.accept().await.unwrap();
                    connection
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 4194305\r\nConnection: close\r\n\r\n",
                        )
                        .await
                        .unwrap();
                });
                let oversized = GrafanaClient::for_test(
                    oversized_origin,
                    std::time::Duration::from_secs(1),
                );
                assert_eq!(
                    oversized
                        .execute(
                            &crate::integrations::grafana::actions::LogqlInput {
                                query: "SensitiveOversizedBodyQuery".to_owned(),
                                start: None,
                                end: None,
                                time: None,
                                direction: None,
                                limit: None,
                            }
                            .validate()
                            .unwrap()
                        )
                        .await,
                    Err(crate::integrations::grafana::Error::InvalidResponse)
                );
                oversized_task.await.unwrap();

                let propagated = propagated.lock().unwrap().clone();
                grafana_task.abort();
                mcp_task.abort();
                propagated
            })
        });

        provider.force_flush().unwrap();
        let spans = exporter.0.lock().unwrap().clone();
        let targets = targets.lock().unwrap().clone();
        TelemetryCapture {
            spans,
            targets,
            propagated,
        }
    }

    #[test]
    fn host_filters_export_grafana_http_hierarchy_and_safe_propagation() {
        const CHILD_MARKER: &str = "HOMELAB_MCP_TELEMETRY_TEST_CHILD";
        if std::env::var_os(CHILD_MARKER).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "mcp::tests::host_filters_export_grafana_http_hierarchy_and_safe_propagation",
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
        let TelemetryCapture {
            spans,
            targets,
            propagated,
        } = capture_authorized_call(&runtime);
        let exported = spans
            .iter()
            .map(|span| (span.name.as_ref(), span.instrumentation_scope.name()))
            .collect::<Vec<_>>();
        let server_spans = spans
            .iter()
            .filter(|span| span.name == "mcp.server.request")
            .collect::<Vec<_>>();
        let grafana_spans = spans
            .iter()
            .filter(|span| span.name == "grafana.query")
            .collect::<Vec<_>>();
        let client_spans = spans
            .iter()
            .filter(|span| span.name == "http.client.request")
            .collect::<Vec<_>>();

        assert!(targets.contains(&("mcp.server.request", "mcp::server")));
        assert!(targets.contains(&("grafana.query", "homelab_mcp::grafana")));
        assert!(targets.contains(&("http.client.request", "homelab_mcp::http_client")));
        assert_eq!(server_spans.len(), 2, "exported spans: {exported:?}");
        assert_eq!(grafana_spans.len(), 7);
        assert_eq!(client_spans.len(), 7);
        for server in &server_spans {
            assert_eq!(server.parent_span_id, SpanId::INVALID);
            let grafana_children = grafana_spans
                .iter()
                .filter(|grafana| {
                    grafana.parent_span_id == server.span_context.span_id()
                        && grafana.span_context.trace_id() == server.span_context.trace_id()
                })
                .collect::<Vec<_>>();
            assert_eq!(grafana_children.len(), 1);
            let client_children = client_spans
                .iter()
                .filter(|client| {
                    client.parent_span_id == grafana_children[0].span_context.span_id()
                        && client.span_context.trace_id()
                            == grafana_children[0].span_context.trace_id()
                })
                .collect::<Vec<_>>();
            assert_eq!(client_children.len(), 1);
        }

        let mut outcomes = Vec::new();
        for client in &client_spans {
            assert_eq!(client.span_kind, SpanKind::Client);
            assert!(grafana_spans.iter().any(|grafana| {
                client.parent_span_id == grafana.span_context.span_id()
                    && client.span_context.trace_id() == grafana.span_context.trace_id()
            }));
            assert!(
                client.attributes.iter().all(|attribute| matches!(
                    attribute.key.as_str(),
                    "code.file.path"
                        | "code.module.name"
                        | "code.line.number"
                        | "thread.id"
                        | "thread.name"
                        | "target"
                        | "http.request.method"
                        | "http.response.status_code"
                        | "http.outcome"
                        | "busy_ns"
                        | "idle_ns"
                )),
                "unexpected client attributes: {:?}",
                client.attributes
            );
            let attributes = client
                .attributes
                .iter()
                .map(|attribute| (attribute.key.as_str(), attribute.value.to_string()))
                .collect::<std::collections::HashMap<_, _>>();
            assert!(matches!(
                attributes["http.outcome"].as_str(),
                "success" | "http_error" | "transport_error" | "response_error" | "cancelled"
            ));
            let status = attributes.get("http.response.status_code").cloned();
            let otel_error = match &client.status {
                Status::Unset => false,
                Status::Error { description } => {
                    assert!(description.is_empty());
                    true
                }
                Status::Ok => panic!("client spans must not override success status"),
            };
            outcomes.push((
                attributes["http.request.method"].clone(),
                attributes["http.outcome"].clone(),
                status,
                otel_error,
            ));
        }
        outcomes.sort();
        assert_eq!(
            outcomes,
            vec![
                ("GET".to_owned(), "cancelled".to_owned(), None, true),
                (
                    "GET".to_owned(),
                    "http_error".to_owned(),
                    Some("503".to_owned()),
                    true
                ),
                (
                    "GET".to_owned(),
                    "response_error".to_owned(),
                    Some("200".to_owned()),
                    true
                ),
                (
                    "GET".to_owned(),
                    "success".to_owned(),
                    Some("200".to_owned()),
                    false
                ),
                ("GET".to_owned(), "transport_error".to_owned(), None, true),
                (
                    "GET".to_owned(),
                    "transport_error".to_owned(),
                    Some("200".to_owned()),
                    true
                ),
                (
                    "POST".to_owned(),
                    "success".to_owned(),
                    Some("200".to_owned()),
                    false
                ),
            ]
        );
        let serialized = format!("{spans:?}");
        for excluded in [
            "sensitive-path",
            "secret=query",
            "SensitiveRawErrorQuery",
            "SensitiveHttpErrorQuery",
            "SensitiveTimeoutQuery",
            "SensitiveTruncatedBodyQuery",
            "SensitiveOversizedBodyQuery",
            "SensitiveMatcher",
            "SensitiveComment",
            "grafana-secret",
            "test-token",
            "error.message",
            "error.cause_chain",
        ] {
            assert!(!serialized.contains(excluded), "leaked {excluded}");
        }
        assert_eq!(
            propagated
                .iter()
                .map(|(method, _)| method.as_str())
                .collect::<Vec<_>>(),
            ["GET", "POST"]
        );
        for (method, traceparent) in propagated {
            let parts = traceparent.split('-').collect::<Vec<_>>();
            assert_eq!(parts.len(), 4, "invalid traceparent: {traceparent}");
            assert_eq!(parts[0], "00");
            assert_eq!(parts[1].len(), 32);
            assert_eq!(parts[2].len(), 16);
            assert_eq!(parts[3], "01");
            assert!(
                parts[1]
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
            );
            assert!(
                parts[2]
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
            );
            let client = client_spans
                .iter()
                .find(|span| {
                    span.span_context.span_id().to_string() == parts[2]
                        && span.attributes.iter().any(|attribute| {
                            attribute.key.as_str() == "http.request.method"
                                && attribute.value.to_string() == method
                        })
                })
                .unwrap();
            assert_eq!(client.span_context.trace_id().to_string(), parts[1]);
        }
    }

    #[tokio::test]
    async fn discovery_list_help_filter_and_call_follow_progressive_contract() {
        let (handler, _, grafana_task) = test_handler().await;
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
        let query_action_enum = query_tool["inputSchema"]["properties"]["action"]["enum"]
            .as_array()
            .unwrap();
        for action in [
            "help",
            "help.logql",
            "help.promql",
            "help.traceql",
            "help.profile",
            "help.alert-rule",
            "help.alert-instance",
            "help.silence",
            "logql.query",
            "promql.query",
            "traceql.search",
            "profile.merge",
            "alert-rule.list",
            "alert-instance.list",
            "silence.list",
        ] {
            assert!(
                query_action_enum.contains(&json!(action)),
                "missing {action}"
            );
        }
        for legacy in [
            "logql",
            "promql",
            "traceql",
            "profiles",
            "alert_rules",
            "alert_instances",
            "list_silences",
        ] {
            assert!(
                !query_action_enum.contains(&json!(legacy)),
                "found {legacy}"
            );
        }
        let exec_action_enum = exec_tool["inputSchema"]["properties"]["action"]["enum"]
            .as_array()
            .unwrap();
        for action in ["help", "help.silence", "silence.create"] {
            assert!(
                exec_action_enum.contains(&json!(action)),
                "missing {action}"
            );
        }
        assert!(!exec_action_enum.contains(&json!("create_silence")));

        let (_, query_root_help) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "query-help",
                json!({"name":QUERY_TOOL_NAME,"arguments":{"action":"help","filter":".namespaces"}}),
            ),
        )
        .await;
        let query_namespaces = query_root_help["result"]["structuredContent"]["result"]
            .as_array()
            .unwrap();
        assert_eq!(
            query_namespaces
                .iter()
                .map(|namespace| namespace["namespace"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec![
                "logql",
                "promql",
                "traceql",
                "profile",
                "alert-rule",
                "alert-instance",
                "silence"
            ]
        );
        let mut query_actions = Vec::new();
        for namespace in [
            "logql",
            "promql",
            "traceql",
            "profile",
            "alert-rule",
            "alert-instance",
            "silence",
        ] {
            let (_, namespace_help) = post_mcp(
                &endpoint,
                request(
                    "tools/call",
                    "namespace-help",
                    json!({
                        "name":QUERY_TOOL_NAME,
                        "arguments":{"action":format!("help.{namespace}"),"filter":".actions"}
                    }),
                ),
            )
            .await;
            query_actions.extend(
                namespace_help["result"]["structuredContent"]["result"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .cloned(),
            );
        }
        assert_eq!(
            query_actions
                .iter()
                .map(|action| action["action"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec![
                "logql.query",
                "promql.query",
                "traceql.search",
                "profile.merge",
                "alert-rule.list",
                "alert-instance.list",
                "silence.list"
            ]
        );
        for (action, required, optional) in [
            (
                "promql.query",
                vec!["query"],
                vec!["start", "end", "step", "time"],
            ),
            (
                "traceql.search",
                vec!["query"],
                vec!["start", "end", "limit"],
            ),
            (
                "profile.merge",
                vec!["selector", "start", "end"],
                vec!["profile_type", "max_nodes"],
            ),
            ("alert-rule.list", vec![], vec!["limit"]),
            ("alert-instance.list", vec![], vec!["matchers", "limit"]),
            ("silence.list", vec![], vec!["state", "limit"]),
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
                json!({"name":EXEC_TOOL_NAME,"arguments":{"action":"help.silence","filter":".actions"}}),
            ),
        )
        .await;
        let exec_actions = exec_help["result"]["structuredContent"]["result"]
            .as_array()
            .unwrap();
        assert_eq!(exec_actions.len(), 1);
        assert_eq!(exec_actions[0]["action"], "silence.create");
        let silence_schema = &exec_actions[0]["input_schema"];
        assert_eq!(silence_schema["additionalProperties"], false);
        assert_eq!(
            silence_schema["required"],
            json!(["matchers", "duration_seconds", "comment"])
        );

        for (action, result_type, expected_field) in [
            ("alert-rule.list", "alert_rules", ("title", "API errors")),
            (
                "alert-instance.list",
                "alert_instances",
                ("fingerprint", "abc123"),
            ),
            ("silence.list", "silences", ("silence_id", "silence-active")),
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
                        "action":"silence.create",
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
                    "arguments":{"action":"alert-instance.list","input":{},"filter":".result[]"}
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
        let (handler, _, grafana_task) = test_handler().await;
        let (origin, mcp_task) = serve(streamable_http_router(handler)).await;
        let endpoint = format!("{origin}/mcp");

        for (tool, action) in [
            (QUERY_TOOL_NAME, "logql"),
            (QUERY_TOOL_NAME, "promql"),
            (QUERY_TOOL_NAME, "traceql"),
            (QUERY_TOOL_NAME, "profiles"),
            (QUERY_TOOL_NAME, "alert_rules"),
            (QUERY_TOOL_NAME, "alert_instances"),
            (QUERY_TOOL_NAME, "list_silences"),
            (EXEC_TOOL_NAME, "create_silence"),
        ] {
            let (_, response) = post_mcp(
                &endpoint,
                request(
                    "tools/call",
                    "legacy-action",
                    json!({"name":tool,"arguments":{"action":action}}),
                ),
            )
            .await;
            assert_eq!(response["error"]["code"], -32602, "{action}: {response}");
        }

        for (tool, arguments) in [
            (QUERY_TOOL_NAME, json!({"action":"unknown"})),
            (QUERY_TOOL_NAME, json!({"action":"silence.create"})),
            (EXEC_TOOL_NAME, json!({"action":"alert-rule.list"})),
            (EXEC_TOOL_NAME, json!({"action":"silence.list"})),
            (
                QUERY_TOOL_NAME,
                json!({"action":"alert-rule.list","input":{"limit":1,"extra":true}}),
            ),
            (
                EXEC_TOOL_NAME,
                json!({
                    "action":"silence.create",
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
                json!({"name":QUERY_TOOL_NAME,"arguments":{"action":"logql.query","input":{"query":" "}}}),
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
                        "action":"silence.create",
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
                        "action":"silence.create",
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
        let (handler, _, grafana_task) = test_handler().await;
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
