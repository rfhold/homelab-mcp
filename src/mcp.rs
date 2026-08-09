#![allow(clippy::useless_vec)]

use std::sync::Arc;

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
    config::Config,
    grafana::{Error as GrafanaError, GrafanaClient},
    logql::LogqlInput,
};

#[cfg(test)]
const TOOL_NAME: &str = "grafana_exec";

#[derive(Clone)]
pub struct HomelabMcp {
    grafana: GrafanaClient,
}

pub fn router(config: &Config, oauth: &OAuthAuthorizationServer) -> Result<Router, String> {
    let handler = Arc::new(HomelabMcp {
        grafana: GrafanaClient::production(
            config.grafana_url.clone(),
            config.grafana_token.clone(),
        )?,
    });
    let required_scope = config.oauth_required_scope.clone();
    let metadata = McpProtectedResourceMetadata::new(
        config.oauth_resource.clone(),
        [config.oauth_issuer.clone()],
    )
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
        name = "grafana_exec",
        description = "Execute bounded, read-only Grafana queries.",
        annotations = json!({
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": true
        })
    )
)]
impl HomelabMcp {
    /// Execute a LogQL instant or range query through Grafana.
    ///
    /// Use instant mode without start/end, or range mode with both endpoints.
    /// Log streams are normalized and limited to the requested number of lines.
    #[action(tool = "grafana_exec", name = "logql")]
    async fn logql(
        &self,
        input: LogqlInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let query = match input.validate() {
            Ok(query) => query,
            Err(()) => return Ok(tool_error(GrafanaError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.grafana.execute(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        match result {
            Ok(output) => Ok(query_result(output)),
            Err(error) => Ok(tool_error(error)),
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

fn tool_error(error: GrafanaError) -> McpToolResult {
    let (code, message, retryable) = match error {
        GrafanaError::InvalidArguments => (
            "invalid_arguments",
            "The LogQL arguments are invalid.",
            false,
        ),
        GrafanaError::CapacityExhausted => (
            "capacity_exhausted",
            "Grafana query capacity is currently exhausted.",
            true,
        ),
        GrafanaError::Timeout => ("timeout", "The Grafana query timed out.", true),
        GrafanaError::Unauthorized => (
            "grafana_unauthorized",
            "Grafana rejected the service credentials.",
            false,
        ),
        GrafanaError::QueryRejected => {
            ("query_rejected", "Grafana rejected the LogQL query.", false)
        }
        GrafanaError::UpstreamUnavailable => (
            "upstream_unavailable",
            "Grafana is currently unavailable.",
            true,
        ),
        GrafanaError::InvalidResponse => (
            "invalid_response",
            "Grafana returned an invalid response.",
            false,
        ),
    };
    McpToolResult::new(json!({
        "content": [{"type":"text","text":message}],
        "structuredContent":{"error":{"code":code,"message":message,"retryable":retryable}},
        "isError": true
    }))
}

#[cfg(test)]
mod tests {
    use axum::{Json, Router, routing::post};
    use mcp::{
        protocol::MCP_PROTOCOL_VERSION,
        server::{
            McpHostedTokenValidation, StreamableHttpAuthorization, StreamableHttpOptions,
            streamable_http_router,
        },
    };
    use reqwest::{Client, StatusCode};
    use serde_json::Value;
    use tokio::{net::TcpListener, task::JoinHandle};

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
        let grafana = Router::new().route(
            "/api/datasources/proxy/uid/loki/loki/api/v1/query",
            post(|| async {
                Json(json!({
                    "status":"success",
                    "data":{"resultType":"vector","result":[{"metric":{"job":"test"},"value":[1786276800,"2"]}]}
                }))
            }),
        );
        let (origin, task) = serve(grafana).await;
        let handler = Arc::new(HomelabMcp {
            grafana: GrafanaClient::for_test(
                url::Url::parse(&format!("{origin}/")).unwrap(),
                std::time::Duration::from_secs(1),
            ),
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
        assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 1);
        assert_eq!(listed["result"]["tools"][0]["name"], TOOL_NAME);
        assert_eq!(
            listed["result"]["tools"][0]["annotations"]["readOnlyHint"],
            true
        );
        assert_eq!(
            listed["result"]["tools"][0]["annotations"]["destructiveHint"],
            false
        );
        assert_eq!(
            listed["result"]["tools"][0]["inputSchema"]["additionalProperties"],
            false
        );

        let (_, help) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "help",
                json!({"name":TOOL_NAME,"arguments":{"action":"help","filter":".actions"}}),
            ),
        )
        .await;
        assert_eq!(
            help["result"]["structuredContent"]["result"][0]["action"],
            "logql"
        );

        let (_, call) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "call",
                json!({"name":TOOL_NAME,"arguments":{"action":"logql","input":{"query":"{job=\"test\"}"}}}),
            ),
        )
        .await;
        assert_eq!(call["result"]["isError"], Value::Null);
        assert_eq!(call["result"]["structuredContent"]["mode"], "instant");
        assert_eq!(call["result"]["structuredContent"]["result_type"], "vector");
        assert!(
            call["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("1 item")
        );

        let (_, filtered_call) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "filtered-call",
                json!({
                    "name":TOOL_NAME,
                    "arguments":{
                        "action":"logql",
                        "input":{"query":"{job=\"test\"}"},
                        "filter":".result[]"
                    }
                }),
            ),
        )
        .await;
        assert!(filtered_call.get("error").is_none(), "{filtered_call}");
        assert_eq!(
            filtered_call["result"]["structuredContent"]["metric"]["job"],
            "test"
        );

        grafana_task.abort();
        mcp_task.abort();
    }

    #[tokio::test]
    async fn malformed_shapes_actions_filters_and_semantic_failures_keep_error_boundary() {
        let (handler, grafana_task) = test_handler().await;
        let (origin, mcp_task) = serve(streamable_http_router(handler)).await;
        let endpoint = format!("{origin}/mcp");
        for arguments in [
            json!({"action":"unknown"}),
            json!({"action":"help","extra":true}),
            json!({"action":"help","filter":".["}),
        ] {
            let (_, response) = post_mcp(
                &endpoint,
                request(
                    "tools/call",
                    "invalid",
                    json!({"name":TOOL_NAME,"arguments":arguments}),
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
                json!({"name":TOOL_NAME,"arguments":{"action":"logql","input":{"query":" "}}}),
            ),
        )
        .await;
        assert_eq!(semantic["result"]["isError"], true);
        assert_eq!(
            semantic["result"]["structuredContent"]["error"],
            json!({"code":"invalid_arguments","message":"The LogQL arguments are invalid.","retryable":false})
        );
        assert!(!semantic.to_string().contains("grafana-secret"));

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
                "Grafana query capacity is currently exhausted.",
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
            let result = tool_error(error).raw;
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
