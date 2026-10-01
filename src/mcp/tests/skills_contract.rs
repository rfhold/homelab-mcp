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
    let expected: [(&str, &[&str]); 4] = [
        (
            "query",
            &[
                "grafana.logql.query",
                "grafana.promql.query",
                "grafana.traceql.search",
                "grafana.profile.merge",
                "grafana.alert-instance.list",
                "grafana.silence.list",
                "grafana.render.dashboard",
                "grafana.render.panel",
                "tekton.run.list",
                "tekton.run.get",
                "tekton.run.status",
                "tekton.run.wait",
                "tekton.task.list",
                "tekton.task.logs",
                "kubernetes.resource_list",
                "kubernetes.resource_get",
                "kubernetes.pod_logs",
                "ceph.status.get",
                "ceph.metrics.summary",
                "ceph.osd.list",
                "ceph.osd.get",
                "ceph.osd.safe-to-destroy",
                "ceph.device.list",
                "ceph.device.get",
                "ceph.flags.get",
                "ceph.task.list",
            ],
        ),
        (
            "create",
            &[
                "grafana.silence.create",
                "kubernetes.cronjob_trigger",
                "machine.create",
            ],
        ),
        (
            "execute",
            &[
                "tekton.workflow.dispatch",
                "tekton.run.rerun",
                "tekton.run.cancel",
                "kubernetes.workload_restart",
                "kubernetes.workload_scale",
                "kubernetes.cronjob_suspend",
                "ceph.osd.mark",
                "ceph.osd.reweight",
                "ceph.osd.scrub",
                "machine.update",
                "machine.host-key.clear",
                "machine.host-key.replace",
                "deploy.run",
            ],
        ),
        (
            "destroy",
            &[
                "kubernetes.pod_delete",
                "ceph.osd.destroy",
                "ceph.osd.purge",
                "machine.delete",
            ],
        ),
    ];
    assert_eq!(tools.len(), expected.len());
    let mut count = 0;
    for (name, actions) in expected {
        let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
        assert_eq!(
            tool["annotations"],
            json!({
                "readOnlyHint": name == "query", "destructiveHint": matches!(name, "execute" | "destroy"),
                "idempotentHint": name == "query", "openWorldHint": true
            })
        );
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
    assert_eq!(count, 46);
    for name in [
        "edit",
        "grafana_query",
        "grafana_exec",
        "grafana_render",
        "tekton_query",
        "tekton_exec",
        "kubernetes_query",
        "kubernetes_exec",
        "ceph_query",
        "ceph_exec",
        "machines",
        "deploys",
    ] {
        let (_, response) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "old-tool",
                json!({"name":name,"arguments":{"action":"list","input":{}}}),
            ),
        )
        .await;
        assert_eq!(response["error"]["code"], -32020, "{response}");
    }
    for action in [
        "kubernetes.cluster_list",
        "kubernetes.capability_list",
        "ceph.cluster.list",
        "grafana.dashboard.list",
        "grafana.dashboard.get",
        "grafana.alert-rule.list",
        "grafana.recording-rule.list",
        "tekton.repository.list",
        "tekton.workflow.list",
        "machine.list",
        "deploy.list",
    ] {
        let (_, response) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "resource-not-tool",
                json!({"name":"query","arguments":{"action":action,"input":{}}}),
            ),
        )
        .await;
        assert_eq!(response["error"]["code"], -32602, "{response}");
    }
    upstream.abort();
    server.abort();
}

#[tokio::test]
async fn resource_discovery_and_error_routes_are_exact() {
    let (handler, _, upstream) = test_handler().await;
    let (origin, server) = serve(streamable_http_router(handler)).await;
    let endpoint = format!("{origin}/mcp");
    let expected = [
        "homelab://kubernetes/clusters",
        "homelab://ceph/clusters",
        "homelab://grafana/dashboards",
        "homelab://grafana/alert-rules",
        "homelab://grafana/recording-rules",
        "homelab://tekton/repositories",
        "homelab://tekton/workflows",
        "homelab://machines",
        "homelab://deploys",
    ];
    let (_, listed) = post_mcp(
        &endpoint,
        request("resources/list", "collections", json!({})),
    )
    .await;
    assert_eq!(
        listed["result"]["resources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|resource| resource["uri"].as_str().unwrap())
            .collect::<Vec<_>>(),
        expected
    );
    let (_, listed) = post_mcp(
        &endpoint,
        request("resources/templates/list", "templates", json!({})),
    )
    .await;
    let mut expected_templates = expected
        .iter()
        .map(|uri| format!("{uri}{{?input,filter}}"))
        .collect::<Vec<_>>();
    expected_templates.extend([
        "homelab://kubernetes/capabilities/{cluster}{?input,filter}".to_owned(),
        "homelab://grafana/dashboards/{uid}{?input,filter}".to_owned(),
    ]);
    assert_eq!(
        listed["result"]["resourceTemplates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|template| template["uriTemplate"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>(),
        expected_templates
    );
    for uri in [
        "homelab://unknown",
        "homelab://machines?input=[]",
        "homelab://machines?input=bad",
        "homelab://machines?input={}&input={}",
        "homelab://machines?limit=1",
        "homelab://machines?input=%7B%22extra%22%3Atrue%7D",
        "homelab://grafana/dashboards?filter=.%5B",
        "homelab://grafana/dashboards/x%2Fy",
        "homelab://grafana/dashboards/dash-1?input=%7B%22uid%22%3A%22dash-1%22%7D",
        "homelab://kubernetes/capabilities/test?input=%7B%22cluster%22%3A%22test%22%7D",
    ] {
        let (_, response) = post_mcp(
            &endpoint,
            request("resources/read", "bad-resource", json!({"uri":uri})),
        )
        .await;
        assert_eq!(response["error"]["code"], -32602, "{uri}: {response}");
        assert!(!response.to_string().contains("grafana-secret"));
    }
    for method in ["resources/list", "resources/templates/list"] {
        let (_, response) = post_mcp(
            &endpoint,
            request(method, "cursor", json!({"cursor":"unknown"})),
        )
        .await;
        assert_eq!(response["error"]["code"], -32602);
    }
    let (_, response) = post_mcp(
        &endpoint,
        request(
            "resources/read",
            "encoded-identity",
            json!({"uri":"homelab://grafana/dashboards/%64ash-1"}),
        ),
    )
    .await;
    assert!(response["result"]["contents"][0]["text"].is_string());
    upstream.abort();
    server.abort();
}

#[tokio::test]
async fn collection_links_are_projectable_and_readable() {
    use crate::integrations::kubernetes::{KubernetesCatalog, KubernetesConfig};
    use std::{fs, os::unix::fs::PermissionsExt};

    let path = std::env::temp_dir().join(format!(
        "homelab-mcp-capability-links-{}",
        std::process::id()
    ));
    fs::write(&path, r#"#!/bin/sh
for argument in "$@"; do
    case "$argument" in
        --raw=/api/*) version=${argument#--raw=/api/} ;;
        --raw=/apis/*) version=${argument#--raw=/apis/} ;;
        *) continue ;;
    esac
    printf '{"kind":"APIResourceList","apiVersion":"v1","groupVersion":"%s","resources":[]}' "$version"
    exit 0
done
exit 1
"#).unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&path, permissions).unwrap();
    let config = KubernetesConfig::new(
        path.clone(),
        "/inert/kubeconfig".into(),
        "/inert/cache".into(),
        "test-context".into(),
        "test".into(),
    );
    let (mut handler, _, upstream) = test_handler().await;
    Arc::get_mut(&mut Arc::get_mut(&mut handler).unwrap().services)
        .unwrap()
        .kubernetes = KubernetesCatalog::new(vec![config]).unwrap();
    let (origin, server) = serve(streamable_http_router(handler)).await;
    let endpoint = format!("{origin}/mcp");
    for (collection, filter, expected_link, result_type) in [
        (
            "homelab://grafana/dashboards",
            ".result[0].resource_uri",
            "homelab://grafana/dashboards/dash-1",
            "dashboard",
        ),
        (
            "homelab://kubernetes/clusters",
            ".result.clusters[0].capabilities_uri",
            "homelab://kubernetes/capabilities/test",
            "capabilities",
        ),
    ] {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("filter", filter)
            .finish();
        let (_, projected) = post_mcp(
            &endpoint,
            request(
                "resources/read",
                "collection-link",
                json!({"uri":format!("{collection}?{query}")}),
            ),
        )
        .await;
        let projected: Value =
            serde_json::from_str(projected["result"]["contents"][0]["text"].as_str().unwrap())
                .unwrap();
        let uri = projected["result"].as_str().unwrap();
        assert_eq!(uri, expected_link);
        let (_, item) = post_mcp(
            &endpoint,
            request("resources/read", "follow-link", json!({"uri":uri})),
        )
        .await;
        let item: Value =
            serde_json::from_str(item["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
        if result_type == "dashboard" {
            assert_eq!(item["result_type"], result_type);
            assert_eq!(item["result"]["uid"], "dash-1");
        } else {
            assert_eq!(item["type"], result_type);
            assert!(
                !item["result"]["capabilities"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        }
    }
    upstream.abort();
    server.abort();
    fs::remove_file(path).unwrap();
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
        ("resources/list", json!({})),
        ("resources/templates/list", json!({})),
        (
            "resources/read",
            json!({"uri":"homelab://grafana/dashboards"}),
        ),
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
