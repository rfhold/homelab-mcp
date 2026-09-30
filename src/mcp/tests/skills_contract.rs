use super::*;
use sha2::{Digest as _, Sha256};

const NAMES: [&str; 5] = [
    "investigate-observability",
    "maintain-ceph",
    "manage-machines",
    "operate-kubernetes",
    "operate-pipelines",
];

#[tokio::test]
async fn skills_http_list_get_and_all_manifest_reads_match_authored_bytes() {
    let (handler, _, upstream) = test_handler().await;
    let (origin, server) = serve(streamable_http_router(handler)).await;
    let endpoint = format!("{origin}/mcp");
    let (_, listed) = post_mcp(&endpoint, request("skills/list", "skills", json!({}))).await;
    assert_eq!(listed["result"]["resultType"], "complete", "{listed}");
    assert_eq!(listed["result"]["cacheScope"], "private");
    assert_eq!(listed["result"]["ttlMs"], 0);
    assert!(listed["result"].get("nextCursor").is_none());
    let skills = listed["result"]["skills"].as_array().unwrap();
    assert_eq!(skills.len(), NAMES.len());
    let mut files_read = 0;
    for (skill, name) in skills.iter().zip(NAMES) {
        let root = format!("skill://homelab/{name}/");
        assert_eq!(skill["uri"], format!("{root}SKILL.md"));
        assert_eq!(skill["frontmatter"]["name"], name);
        let (_, got) = post_mcp(
            &endpoint,
            request("skills/get", "get", json!({"uri":skill["uri"]})),
        )
        .await;
        assert_eq!(got["result"]["resultType"], "complete");
        assert_eq!(got["result"]["cacheScope"], "private");
        assert_eq!(got["result"]["ttlMs"], 0);
        assert_eq!(got["result"]["skill"], *skill);
        let files = skill["resources"].as_array().unwrap();
        assert_eq!(files.len(), 2);
        for (file, relative) in files.iter().zip(["SKILL.md", "references/actions.md"]) {
            assert_eq!(file["uri"], format!("{root}{relative}"));
            let (_, read) = post_mcp(
                &endpoint,
                request("resources/read", "read", json!({"uri":file["uri"]})),
            )
            .await;
            let contents = read["result"]["contents"]
                .as_array()
                .unwrap_or_else(|| panic!("{read}"));
            assert_eq!(contents.len(), 1, "{read}");
            assert_eq!(contents[0]["uri"], file["uri"]);
            let bytes = contents[0]["text"].as_str().unwrap().as_bytes();
            let authored = std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("src/skills")
                    .join(name)
                    .join(relative),
            )
            .unwrap();
            assert_eq!(bytes, authored);
            assert_eq!(file["size"], bytes.len() as u64);
            assert_eq!(
                file["digest"],
                format!(
                    "sha256:{}",
                    Sha256::digest(bytes)
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                )
            );
            if relative == "SKILL.md" {
                assert_eq!(
                    skill["frontmatter"],
                    json!(mcp::skills::parse_skill_frontmatter(bytes).unwrap())
                );
                let text = std::str::from_utf8(bytes).unwrap();
                assert!(text.contains("(references/actions.md)"));
            }
            files_read += 1;
        }
    }
    assert_eq!(files_read, 10);
    for (method, params, code) in [
        (
            "skills/get",
            json!({"uri":"skill://homelab/unknown/SKILL.md"}),
            -32602,
        ),
        (
            "resources/read",
            json!({"uri":"skill://homelab/manage-machines/missing.md"}),
            -32602,
        ),
        ("skills/list", json!({"cursor":"unknown"}), -32602),
    ] {
        let (_, response) = post_mcp(&endpoint, request(method, "unknown", params)).await;
        assert_eq!(response["error"]["code"], code, "{response}");
        assert!(response.get("result").is_none());
    }
    upstream.abort();
    server.abort();
}

#[tokio::test]
async fn domain_action_enums_are_exact_and_all_generated_help_calls_are_rejected() {
    let (handler, _, upstream) = test_handler().await;
    let (origin, server) = serve(streamable_http_router(handler)).await;
    let endpoint = format!("{origin}/mcp");
    let (_, listed) = post_mcp(&endpoint, request("tools/list", "tools", json!({}))).await;
    let tools = listed["result"]["tools"].as_array().unwrap();
    let expected: [(&str, &[&str]); 11] = [
        (
            QUERY_TOOL_NAME,
            &[
                "logql.query",
                "promql.query",
                "traceql.search",
                "profile.merge",
                "alert-rule.list",
                "recording-rule.list",
                "alert-instance.list",
                "silence.list",
                "dashboard.list",
                "dashboard.get",
            ],
        ),
        (RENDER_TOOL_NAME, &["dashboard", "panel"]),
        (EXEC_TOOL_NAME, &["silence.create"]),
        (
            TEKTON_QUERY_TOOL_NAME,
            &[
                "repository.list",
                "workflow.list",
                "run.list",
                "run.get",
                "run.status",
                "run.wait",
                "task.list",
                "task.logs",
            ],
        ),
        (
            TEKTON_EXEC_TOOL_NAME,
            &["workflow.dispatch", "run.rerun", "run.cancel"],
        ),
        (
            KUBERNETES_QUERY_TOOL_NAME,
            &[
                "cluster_list",
                "capability_list",
                "resource_list",
                "resource_get",
                "pod_logs",
            ],
        ),
        (
            KUBERNETES_EXEC_TOOL_NAME,
            &[
                "workload_restart",
                "workload_scale",
                "cronjob_suspend",
                "cronjob_trigger",
                "pod_delete",
            ],
        ),
        (
            CEPH_QUERY_TOOL_NAME,
            &[
                "cluster.list",
                "status.get",
                "metrics.summary",
                "osd.list",
                "osd.get",
                "osd.safe-to-destroy",
                "device.list",
                "device.get",
                "flags.get",
                "task.list",
            ],
        ),
        (
            CEPH_EXEC_TOOL_NAME,
            &[
                "osd.mark",
                "osd.reweight",
                "osd.scrub",
                "osd.destroy",
                "osd.purge",
            ],
        ),
        (
            MACHINES_TOOL_NAME,
            &[
                "list",
                "create",
                "update",
                "delete",
                "host-key.clear",
                "host-key.replace",
            ],
        ),
        (DEPLOYS_TOOL_NAME, &["list", "run"]),
    ];
    assert_eq!(tools.len(), expected.len());
    let mut count = 0;
    for (name, actions) in expected {
        let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
        assert_eq!(
            tool["inputSchema"]["properties"]["action"]["enum"],
            json!(actions)
        );
        count += actions.len();
        for action in [
            "help",
            "help.osd",
            "help.silence",
            "help.host-key",
            "help.run",
        ] {
            let (_, response) = post_mcp(
                &endpoint,
                request(
                    "tools/call",
                    "removed-help",
                    json!({"name":name,"arguments":{"action":action}}),
                ),
            )
            .await;
            assert_eq!(response["error"]["code"], -32602, "{response}");
        }
    }
    assert_eq!(count, 57);
    upstream.abort();
    server.abort();
}

#[tokio::test]
async fn skills_and_catalog_reads_share_global_scope_and_origin_protection() {
    let scopes = [
        "mcp:use",
        "kubernetes:read",
        "kubernetes:write",
        "inventory:read",
        "inventory:write",
        "inventory:host-trust",
        "deploy:read",
        "deploy:run",
    ];
    let (handler, _, upstream) = test_handler().await;
    let metadata = McpProtectedResourceMetadata::new(
        "http://127.0.0.1/mcp",
        ["https://auth.example.test/oauth"],
    )
    .with_scopes(scopes);
    let authorization = StreamableHttpAuthorization::hosted(metadata, move |token, context| {
        assert_eq!(context.required_scopes, scopes.map(str::to_owned));
        Box::pin(async move {
            match token.0.as_str() {
                "valid" => McpHostedTokenValidation::Authorized(McpTokenAuthorization {
                    principal_id: McpPrincipalId::new("skills-test").unwrap(),
                    expires_at: None,
                    revocation: None,
                }),
                "wrong-scopes" => McpHostedTokenValidation::InsufficientScope {
                    required_scopes: scopes.map(str::to_owned).to_vec(),
                    error_description: None,
                },
                _ => McpHostedTokenValidation::Unauthorized {
                    error_description: None,
                },
            }
        })
    })
    .unwrap()
    .with_required_scopes(scopes);
    let (origin, server) = serve(streamable_http_router_with_options(
        handler,
        StreamableHttpOptions::default().with_authorization(authorization),
    ))
    .await;
    let endpoint = format!("{origin}/mcp");
    for (method, params) in [
        ("skills/list", json!({})),
        (
            "skills/get",
            json!({"uri":"skill://homelab/manage-machines/SKILL.md"}),
        ),
        (
            "resources/read",
            json!({"uri":"skill://homelab/manage-machines/SKILL.md"}),
        ),
        (
            "resources/read",
            json!({"uri":"skill://homelab/manage-machines/references/actions.md"}),
        ),
    ] {
        for (token, bad_origin, expected) in [
            (None, false, StatusCode::UNAUTHORIZED),
            (Some("invalid"), false, StatusCode::UNAUTHORIZED),
            (Some("wrong-scopes"), false, StatusCode::FORBIDDEN),
            (Some("valid"), true, StatusCode::FORBIDDEN),
            (Some("valid"), false, StatusCode::OK),
        ] {
            let mut call = Client::new()
                .post(&endpoint)
                .header("accept", "application/json, text/event-stream")
                .header("content-type", "application/json")
                .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
                .header("mcp-method", method);
            if let Some(token) = token {
                call = call.bearer_auth(token);
            }
            if method == "resources/read" {
                call = call.header("mcp-name", params["uri"].as_str().unwrap());
            }
            if bad_origin {
                call = call.header("origin", "https://attacker.example");
            }
            let response = call
                .json(&request(method, "protected-skills", params.clone()))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), expected, "{method} {token:?}");
            if expected != StatusCode::OK {
                let text = response.text().await.unwrap();
                assert!(!text.contains("# Manage Machines"));
                assert!(!text.contains("sha256:"));
            }
        }
    }
    upstream.abort();
    server.abort();
}
