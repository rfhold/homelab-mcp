use super::*;
use mcp::server::{BoxFuture, ServerCapabilities, ServerHandler, ServerInfo};
use mcp::{McpResourceList, McpResourceResult, McpResourceTemplateList, McpToolCall, McpToolList};
use serde::de::DeserializeOwned;
use serde_json::Value;

pub(super) struct ResourceHandler(pub Arc<HomelabMcp>);

const PATH_SEGMENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

const COLLECTIONS: &[(&str, &str, &str)] = &[
    (
        "homelab://kubernetes/clusters",
        "Kubernetes clusters",
        "Configured cluster catalog; no live object state.",
    ),
    (
        "homelab://ceph/clusters",
        "Ceph clusters",
        "Configured Ceph Dashboard cluster catalog.",
    ),
    (
        "homelab://grafana/dashboards",
        "Grafana dashboards",
        "Bounded normalized dashboard inventory.",
    ),
    (
        "homelab://grafana/alert-rules",
        "Grafana alert rules",
        "Bounded normalized alert-rule definitions; not current alerts.",
    ),
    (
        "homelab://grafana/recording-rules",
        "Grafana recording rules",
        "Bounded normalized recording-rule definitions.",
    ),
    (
        "homelab://tekton/repositories",
        "Tekton repositories",
        "Configured PAC repository definitions.",
    ),
    (
        "homelab://tekton/workflows",
        "Tekton workflows",
        "Bounded PAC workflow definitions; not runs.",
    ),
    (
        "homelab://machines",
        "Machines",
        "Bounded machine inventory and public host-trust status.",
    ),
    (
        "homelab://deploys",
        "Deploys",
        "Approved deploy definitions; no execution.",
    ),
];

const TEMPLATES: &[(&str, &str, &str)] = &[
    (
        "homelab://kubernetes/capabilities/{cluster}{?input,filter}",
        "Kubernetes capabilities",
        "Approved kinds for an exact configured cluster. input is percent-encoded JSON; optional kinds array.",
    ),
    (
        "homelab://grafana/dashboards/{uid}{?input,filter}",
        "Grafana dashboard",
        "Bounded dashboard definition for an exact UID; no raw manifests or queries.",
    ),
];

impl ServerHandler for ResourceHandler {
    fn skill_catalog(&self) -> Option<Arc<SkillCatalog>> {
        Some(Arc::clone(&self.0.catalog))
    }
    fn server_info(&self) -> ServerInfo {
        ServerInfo::new("homelab-mcp", "0.1.0")
            .with_description("Authenticated homelab resources and operations.")
    }
    fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities::new().tools().resources().skills()
    }
    fn instructions(&self) -> Option<String> {
        Some("Discover configuration catalogs with resources/list and resources/templates/list. Read homelab:// resources with optional percent-encoded JSON input and jq filter query parameters. Live state, logs, runs and observability remain query actions. Mutations use create, execute or destroy; no text-editable source is exposed.".into())
    }
    fn list_tools(
        &self,
        cursor: Option<String>,
        _: ServerContext,
    ) -> BoxFuture<ServerResult<McpToolList>> {
        Box::pin(async move {
            reject_cursor(cursor)?;
            Ok(super::tools::list())
        })
    }
    fn call_tool(
        &self,
        call: McpToolCall,
        context: ServerContext,
    ) -> BoxFuture<ServerResult<McpToolResult>> {
        super::tools::call(Arc::clone(&self.0), call, context)
    }
    fn list_resources(
        &self,
        cursor: Option<String>,
        _: ServerContext,
    ) -> BoxFuture<ServerResult<McpResourceList>> {
        Box::pin(async move {
            reject_cursor(cursor)?;
            Ok(McpResourceList {
                resources: COLLECTIONS
                    .iter()
                    .map(|(uri, name, description)| {
                        mcp::progressive::action_resource_definition(uri, name, description)
                    })
                    .collect(),
                next_cursor: None,
            })
        })
    }
    fn list_resource_templates(
        &self,
        cursor: Option<String>,
        _: ServerContext,
    ) -> BoxFuture<ServerResult<McpResourceTemplateList>> {
        Box::pin(async move {
            reject_cursor(cursor)?;
            let mut resource_templates = COLLECTIONS
                .iter()
                .map(|(uri, name, description)| {
                    let description = format!(
                        "{description} input is percent-encoded JSON; filter is an optional jq projection. Input schema: {}",
                        input_schema(uri)
                    );
                    mcp::progressive::action_resource_template_definition(
                        &format!("{uri}{{?input,filter}}"), name, &description
                    )
                })
                .collect::<Vec<_>>();
            resource_templates.extend(TEMPLATES.iter().map(|(uri, name, description)| {
                let description = format!(
                    "{description} Identity is bound by the path and must not be repeated in input. Input schema: {}",
                    input_schema(uri)
                );
                mcp::progressive::action_resource_template_definition(uri, name, &description)
            }));
            Ok(McpResourceTemplateList {
                resource_templates,
                next_cursor: None,
            })
        })
    }
    fn read_resource(
        &self,
        uri: String,
        context: ServerContext,
    ) -> BoxFuture<ServerResult<McpResourceResult>> {
        let handler = self.0.clone();
        Box::pin(async move {
            if let Some(result) = handler.catalog.read(&uri) {
                return Ok(result);
            }
            let (route, mut input, filter) = parse_uri(&uri)?;
            macro_rules! read {
                ($method:ident) => {
                    handler.$method(decode(input)?, context).await?
                };
            }
            let result = match route.as_str() {
                "kubernetes/clusters" => read!(kubernetes_clusters),
                "ceph/clusters" => read!(ceph_clusters),
                "grafana/dashboards" => read!(list_dashboards),
                "grafana/alert-rules" => read!(alert_rules),
                "grafana/recording-rules" => read!(recording_rules),
                "tekton/repositories" => read!(tekton_repositories),
                "tekton/workflows" => read!(tekton_workflows),
                "machines" => read!(machine_list),
                "deploys" => read!(deploy_list),
                _ if route.starts_with("kubernetes/capabilities/") => {
                    bind(
                        &mut input,
                        "cluster",
                        &route["kubernetes/capabilities/".len()..],
                    )?;
                    read!(kubernetes_capabilities)
                }
                _ if route.starts_with("grafana/dashboards/") => {
                    bind(&mut input, "uid", &route["grafana/dashboards/".len()..])?;
                    read!(get_dashboard)
                }
                _ => return Err(ServerError::resource_not_found(uri)),
            };
            if result.raw["isError"] == true {
                return Err(ServerError::invalid_params(
                    result.raw["structuredContent"].to_string(),
                ));
            }
            let mut value = result
                .raw
                .get("structuredContent")
                .cloned()
                .ok_or_else(|| ServerError::internal("resource output unavailable"))?;
            enrich_collection(&route, &mut value);
            let projected = mcp::progressive::tool_result(value, filter.as_deref())?;
            mcp::progressive::action_resource_result(uri, projected)
        })
    }
}

fn enrich_collection(route: &str, value: &mut Value) {
    let (items, identity, field, prefix) = match route {
        "grafana/dashboards" => (
            value.get_mut("result"),
            "uid",
            "resource_uri",
            "homelab://grafana/dashboards/",
        ),
        "kubernetes/clusters" => (
            value.pointer_mut("/result/clusters"),
            "name",
            "capabilities_uri",
            "homelab://kubernetes/capabilities/",
        ),
        _ => return,
    };
    let Some(items) = items.and_then(Value::as_array_mut) else {
        return;
    };
    for item in items {
        let Some(identity) = item.get(identity).and_then(Value::as_str) else {
            continue;
        };
        let valid = if route == "grafana/dashboards" {
            GetDashboardInput {
                uid: identity.to_owned(),
            }
            .validate()
            .is_ok()
        } else {
            CapabilityListInput {
                cluster: identity.to_owned(),
                kinds: None,
            }
            .validate()
            .is_ok()
        };
        if valid {
            let uri = format!(
                "{prefix}{}",
                percent_encoding::utf8_percent_encode(identity, PATH_SEGMENT)
            );
            item[field] = json!(uri);
        }
    }
}

fn reject_cursor(cursor: Option<String>) -> ServerResult<()> {
    if cursor.is_some() {
        return Err(ServerError::invalid_params("unknown resource cursor"));
    }
    Ok(())
}

fn decode<T: DeserializeOwned>(input: Value) -> ServerResult<T> {
    serde_json::from_value(input).map_err(|_| ServerError::invalid_params("invalid resource input"))
}

fn input_schema(uri: &str) -> Value {
    let mut schema = match uri {
        "homelab://kubernetes/clusters" => mcp::progressive::json_schema_for::<ClusterListInput>(),
        "homelab://ceph/clusters" => mcp::progressive::json_schema_for::<CephClusterListInput>(),
        "homelab://grafana/dashboards" => {
            mcp::progressive::json_schema_for::<ListDashboardsInput>()
        }
        "homelab://grafana/alert-rules" => mcp::progressive::json_schema_for::<AlertRulesInput>(),
        "homelab://grafana/recording-rules" => {
            mcp::progressive::json_schema_for::<RecordingRulesInput>()
        }
        "homelab://tekton/repositories" => {
            mcp::progressive::json_schema_for::<RepositoryListInput>()
        }
        "homelab://tekton/workflows" => mcp::progressive::json_schema_for::<WorkflowListInput>(),
        "homelab://machines" => mcp::progressive::json_schema_for::<MachineListInput>(),
        "homelab://deploys" => mcp::progressive::json_schema_for::<DeployListInput>(),
        _ if uri.starts_with("homelab://kubernetes/capabilities/") => {
            mcp::progressive::json_schema_for::<CapabilityListInput>()
        }
        _ => mcp::progressive::json_schema_for::<GetDashboardInput>(),
    };
    let identity = if uri.starts_with("homelab://kubernetes/capabilities/") {
        Some("cluster")
    } else if uri.starts_with("homelab://grafana/dashboards/") {
        Some("uid")
    } else {
        None
    };
    if let Some(identity) = identity {
        if let Some(properties) = schema["properties"].as_object_mut() {
            properties.remove(identity);
        }
        if let Some(required) = schema["required"].as_array_mut() {
            required.retain(|field| field != identity);
        }
    }
    schema
}

fn bind(input: &mut Value, key: &str, encoded: &str) -> ServerResult<()> {
    let decoded = percent_encoding::percent_decode_str(encoded)
        .decode_utf8()
        .map_err(|_| ServerError::invalid_params("invalid resource identity"))?;
    if decoded.is_empty() || decoded.contains('/') || input.get(key).is_some() {
        return Err(ServerError::invalid_params(
            "invalid or conflicting resource identity",
        ));
    }
    input[key] = json!(decoded);
    Ok(())
}

fn parse_uri(uri: &str) -> ServerResult<(String, Value, Option<String>)> {
    if uri.len() > 8192 {
        return Err(ServerError::invalid_params(
            "resource URI exceeds 8192 bytes",
        ));
    }
    if !uri.starts_with("homelab://") || uri.contains('#') {
        return Err(ServerError::resource_not_found(uri));
    }
    let (route, query) = uri["homelab://".len()..]
        .split_once('?')
        .unwrap_or((&uri["homelab://".len()..], ""));
    let mut input = None;
    let mut filter = None;
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "input" if input.is_none() => {
                let value: Value = serde_json::from_str(&value)
                    .map_err(|_| ServerError::invalid_params("invalid resource input JSON"))?;
                if !value.is_object() {
                    return Err(ServerError::invalid_params(
                        "resource input must be an object",
                    ));
                }
                input = Some(value);
            }
            "filter" if filter.is_none() => filter = Some(value.into_owned()),
            _ => {
                return Err(ServerError::invalid_params(
                    "unknown or duplicate resource parameter",
                ));
            }
        }
    }
    Ok((route.to_owned(), input.unwrap_or_else(|| json!({})), filter))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_links_preserve_fields_and_skip_unsupported_identities() {
        let original = json!({"mode":"list", "result":[{"uid":"dash-1", "title":"Overview", "tags":["prod"]}, {"uid":"x/y"}, {"uid":"x".repeat(41)}, {"uid":42}]});
        let mut value = original.clone();
        enrich_collection("grafana/dashboards", &mut value);
        assert_eq!(
            value["result"][0]["resource_uri"],
            "homelab://grafana/dashboards/dash-1"
        );
        value["result"][0]
            .as_object_mut()
            .unwrap()
            .remove("resource_uri");
        assert_eq!(value, original);
        let mut value = json!({"result":[{"uid":"dash-1"}]});
        let original = value.clone();
        enrich_collection("grafana/alert-rules", &mut value);
        assert_eq!(value, original);
        let mut value = json!({"result":{"clusters":[{"name":"test", "context":"test-context"}], "metadata":{"returned":1}}});
        let original = value.clone();
        enrich_collection("kubernetes/clusters", &mut value);
        assert_eq!(
            value["result"]["clusters"][0]["capabilities_uri"],
            "homelab://kubernetes/capabilities/test"
        );
        value["result"]["clusters"][0]
            .as_object_mut()
            .unwrap()
            .remove("capabilities_uri");
        assert_eq!(value, original);
    }

    #[test]
    fn uri_parameters_are_bounded_and_unambiguous() {
        for uri in [
            "https://grafana/dashboards",
            "homelab://machines#x",
            "homelab://machines?unknown=x",
            "homelab://machines?input=[]",
            "homelab://machines?input=",
            "homelab://machines?input={}&input={}",
            "homelab://machines?filter=x&filter=y",
        ] {
            assert!(parse_uri(uri).is_err(), "{uri}");
        }
        assert!(parse_uri(&format!("homelab://machines?filter={}", "x".repeat(8192))).is_err());
        let (_, input, filter) = parse_uri(
            "homelab://machines?input=%7B%22limit%22%3A1%7D&filter=.machines%5B%5D.display_name",
        )
        .unwrap();
        assert_eq!(input, json!({"limit":1}));
        assert_eq!(filter.as_deref(), Some(".machines[].display_name"));
    }

    #[test]
    fn path_identity_cannot_override_input_or_escape_its_scope() {
        for identity in ["", "a/b", "a%2Fb", "%FF"] {
            assert!(bind(&mut json!({}), "uid", identity).is_err());
        }
        assert!(bind(&mut json!({"uid":"x"}), "uid", "x").is_err());
        let mut input = json!({});
        bind(&mut input, "uid", "%64ash-1").unwrap();
        assert_eq!(input, json!({"uid":"dash-1"}));
        for (uri, identity) in [(TEMPLATES[0].0, "cluster"), (TEMPLATES[1].0, "uid")] {
            let schema = input_schema(uri);
            assert!(schema["properties"].get(identity).is_none());
            assert!(
                !schema["required"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|field| field == identity)
            );
        }
    }
}
