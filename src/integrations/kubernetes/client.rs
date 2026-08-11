use std::{
    collections::{BTreeMap, HashSet},
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chrono::{SecondsFormat, Utc};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::Value;

use super::{
    Error,
    actions::{CapabilitiesQuery, ExecCommand, ResourceKind, ResourceQuery, valid_catalog_name},
    normalize::{
        self, Capability, CapabilityCatalog, Cluster, ExecResult, ListMetadata, QueryResult,
        ResourceList,
    },
    runner::{Operation, Runner},
};

const MAX_PAGES: usize = 5;
const MAX_INSPECTED: usize = 500;
static TRIGGER_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct KubernetesConfig {
    pub kubectl_executable: PathBuf,
    pub kubeconfig: PathBuf,
    pub cache_dir: PathBuf,
    pub context: String,
    pub cluster_name: String,
    pub deadline: Duration,
}

impl KubernetesConfig {
    pub fn new(
        kubectl_executable: PathBuf,
        kubeconfig: PathBuf,
        cache_dir: PathBuf,
        context: String,
        cluster_name: String,
    ) -> Self {
        Self {
            kubectl_executable,
            kubeconfig,
            cache_dir,
            context,
            cluster_name,
            deadline: Duration::from_secs(30),
        }
    }
}

#[derive(Clone)]
pub struct KubernetesClient {
    runner: Runner,
    context: String,
    cluster_name: String,
}

impl KubernetesClient {
    pub fn new(config: KubernetesConfig) -> Result<Self, String> {
        if !valid_catalog_name(&config.cluster_name) {
            return Err("invalid Kubernetes cluster name".into());
        }
        let runner = Runner::new(
            config.kubectl_executable,
            config.kubeconfig,
            config.cache_dir,
            config.context.clone(),
            config.deadline,
        )?;
        Ok(Self {
            runner,
            context: config.context,
            cluster_name: config.cluster_name,
        })
    }

    pub(crate) fn cluster(&self) -> Cluster {
        Cluster {
            name: self.cluster_name.clone(),
            context: self.context.clone(),
        }
    }

    #[cfg(test)]
    pub async fn capabilities(&self, query: &CapabilitiesQuery) -> Result<QueryResult, Error> {
        let mut cancellation = Box::pin(std::future::pending());
        self.capabilities_cancelled(query, cancellation.as_mut())
            .await
    }

    pub(crate) async fn capabilities_cancelled(
        &self,
        query: &CapabilitiesQuery,
        mut cancellation: Pin<&mut (dyn Future<Output = ()> + Send)>,
    ) -> Result<QueryResult, Error> {
        if !query.is_valid() {
            return Err(Error::InvalidArguments);
        }
        let mut endpoints = BTreeMap::<String, Option<Vec<DiscoveredResource>>>::new();
        let mut capabilities = Vec::with_capacity(query.kinds.len());
        let mut pages = 0_u8;
        for kind in &query.kinds {
            let (prefix, version, resource_name) = kind.mapping();
            let path = format!("/{prefix}/{version}");
            if !endpoints.contains_key(&path) {
                pages += 1;
                let resources = match self
                    .raw_cancelled(&path, Operation::Query, cancellation.as_mut())
                    .await
                {
                    Ok(bytes) => Some(parse_discovery(&bytes, version)?),
                    Err(Error::QueryRejected | Error::NotFound | Error::UnsupportedApi) => None,
                    Err(error) => return Err(error),
                };
                endpoints.insert(path.clone(), resources);
            }
            let supported =
                endpoints
                    .get(&path)
                    .and_then(Option::as_ref)
                    .is_some_and(|resources| {
                        resources.iter().any(|resource| {
                            resource.name == resource_name
                                && resource.namespaced == kind.namespaced()
                        })
                    });
            capabilities.push(Capability {
                kind: *kind,
                api_version: kind.api_version(),
                supported,
            });
        }
        let count = capabilities.len() as u16;
        Ok(QueryResult::Capabilities(CapabilityCatalog {
            capabilities,
            metadata: ListMetadata {
                pages,
                inspected: count,
                returned: count,
                source_truncated: false,
                result_truncated: false,
                aggregate_truncated: false,
                truncated: false,
            },
        }))
    }

    #[cfg(test)]
    pub async fn query(&self, query: &ResourceQuery) -> Result<QueryResult, Error> {
        let mut cancellation = Box::pin(std::future::pending());
        self.query_cancelled(query, cancellation.as_mut()).await
    }

    pub(crate) async fn query_cancelled(
        &self,
        query: &ResourceQuery,
        mut cancellation: Pin<&mut (dyn Future<Output = ()> + Send)>,
    ) -> Result<QueryResult, Error> {
        if !query.is_valid() {
            return Err(Error::InvalidArguments);
        }
        if let Some(name) = &query.name {
            let path = object_path(query.kind, query.namespace.as_deref(), Some(name))?;
            let value = parse_json(
                &self
                    .raw_cancelled(&path, Operation::Query, cancellation.as_mut())
                    .await?,
            )?;
            let resource = normalize::resource(query.kind, &value)?;
            if resource.name != *name || resource.namespace.as_deref() != query.namespace.as_deref()
            {
                return Err(Error::InvalidResponse);
            }
            return Ok(QueryResult::Resource(resource));
        }
        self.list_cancelled(query, cancellation.as_mut())
            .await
            .map(QueryResult::Resources)
    }

    pub(crate) async fn execute_cancelled(
        &self,
        command: &ExecCommand,
        cancellation: Pin<&mut (dyn Future<Output = ()> + Send)>,
    ) -> Result<ExecResult, Error> {
        if !command.is_valid() || command.cluster() != self.cluster_name {
            return Err(Error::MutationRejected);
        }
        let (arguments, action, namespace, name, dry_run) = mutation_arguments(command);
        self.runner
            .run_cancelled(arguments, Operation::Mutation, cancellation)
            .await?;
        Ok(ExecResult {
            status: if dry_run { "validated" } else { "accepted" }.into(),
            action,
            namespace,
            name,
            dry_run,
        })
    }

    #[cfg(test)]
    async fn list(&self, query: &ResourceQuery) -> Result<ResourceList, Error> {
        let mut cancellation = Box::pin(std::future::pending());
        self.list_cancelled(query, cancellation.as_mut()).await
    }

    async fn list_cancelled(
        &self,
        query: &ResourceQuery,
        mut cancellation: Pin<&mut (dyn Future<Output = ()> + Send)>,
    ) -> Result<ResourceList, Error> {
        let base = object_path(query.kind, query.namespace.as_deref(), None)?;
        let selector = selector(&query.labels);
        let mut continuation: Option<String> = None;
        let mut seen = HashSet::new();
        let mut items = Vec::new();
        let mut pages = 0;
        let mut source_truncated = false;
        loop {
            let remaining = MAX_INSPECTED - items.len();
            if remaining == 0 {
                source_truncated = continuation.is_some();
                break;
            }
            let requested = remaining.min(100);
            let mut path = format!("{base}?limit={requested}");
            if let Some(selector) = &selector {
                path.push_str("&labelSelector=");
                path.push_str(&encode(selector));
            }
            if let Some(token) = &continuation {
                path.push_str("&continue=");
                path.push_str(&encode(token));
            }
            let wrapper = parse_json(
                &self
                    .raw_cancelled(&path, Operation::Query, cancellation.as_mut())
                    .await?,
            )?;
            pages += 1;
            let page = wrapper
                .get("items")
                .and_then(Value::as_array)
                .ok_or(Error::InvalidResponse)?;
            let metadata = wrapper
                .get("metadata")
                .and_then(Value::as_object)
                .ok_or(Error::InvalidResponse)?;
            if page.len() > requested {
                return Err(Error::InvalidResponse);
            }
            items.extend(page.iter().take(remaining).cloned());
            continuation = match metadata.get("continue") {
                None | Some(Value::Null) => None,
                Some(Value::String(value)) if value.is_empty() => None,
                Some(Value::String(value))
                    if value.len() <= 4096 && !value.chars().any(char::is_control) =>
                {
                    Some(value.clone())
                }
                Some(_) => return Err(Error::InvalidResponse),
            };
            let Some(token) = continuation.as_ref() else {
                break;
            };
            if !seen.insert(token.clone()) {
                return Err(Error::InvalidResponse);
            }
            if pages >= MAX_PAGES {
                source_truncated = true;
                break;
            }
        }
        let inspected = items.len();
        let mut normalized = items
            .iter()
            .map(|item| normalize::resource(query.kind, item))
            .collect::<Result<Vec<_>, _>>()?;
        normalized.sort_by(|a, b| {
            a.namespace
                .cmp(&b.namespace)
                .then_with(|| a.name.cmp(&b.name))
        });
        let result_truncated = normalized.len() > usize::from(query.limit);
        normalized.truncate(usize::from(query.limit));
        let aggregate_truncated =
            query.kind == ResourceKind::Event && normalize::bound_event_messages(&mut normalized);
        Ok(ResourceList {
            kind: query.kind,
            metadata: ListMetadata {
                pages: pages as u8,
                inspected: inspected as u16,
                returned: normalized.len() as u16,
                source_truncated,
                result_truncated,
                aggregate_truncated,
                truncated: source_truncated || result_truncated || aggregate_truncated,
            },
            items: normalized,
        })
    }

    async fn raw_cancelled(
        &self,
        path: &str,
        operation: Operation,
        cancellation: Pin<&mut (dyn Future<Output = ()> + Send)>,
    ) -> Result<Vec<u8>, Error> {
        self.runner
            .run_cancelled(
                vec!["get".into(), format!("--raw={path}")],
                operation,
                cancellation,
            )
            .await
            .map(|output| output.stdout)
    }
}

fn object_path(
    kind: ResourceKind,
    namespace: Option<&str>,
    name: Option<&str>,
) -> Result<String, Error> {
    if kind.namespaced() && namespace.is_none() && name.is_some() {
        return Err(Error::InvalidArguments);
    }
    let (prefix, version, resource) = kind.mapping();
    let mut path = format!("/{prefix}/{version}");
    if kind.namespaced()
        && let Some(namespace) = namespace
    {
        path.push_str("/namespaces/");
        path.push_str(namespace);
    }
    path.push('/');
    path.push_str(resource);
    if let Some(name) = name {
        path.push('/');
        path.push_str(name);
    }
    Ok(path)
}

fn selector(labels: &BTreeMap<String, String>) -> Option<String> {
    (!labels.is_empty()).then(|| {
        labels
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(",")
    })
}

fn encode(value: &str) -> String {
    utf8_percent_encode(value, NON_ALPHANUMERIC).to_string()
}
fn parse_json(bytes: &[u8]) -> Result<Value, Error> {
    serde_json::from_slice(bytes).map_err(|_| Error::InvalidResponse)
}

struct DiscoveredResource {
    name: String,
    namespaced: bool,
}

fn parse_discovery(bytes: &[u8], expected_version: &str) -> Result<Vec<DiscoveredResource>, Error> {
    let value = parse_json(bytes)?;
    if value.get("groupVersion").and_then(Value::as_str) != Some(expected_version) {
        return Err(Error::InvalidResponse);
    }
    let resources = value
        .get("resources")
        .and_then(Value::as_array)
        .ok_or(Error::InvalidResponse)?;
    if resources.len() > 1_000 {
        return Err(Error::InvalidResponse);
    }
    let mut seen = HashSet::new();
    resources
        .iter()
        .map(|resource| {
            let name = resource
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| {
                    !name.is_empty() && name.len() <= 253 && !name.chars().any(char::is_control)
                })
                .ok_or(Error::InvalidResponse)?;
            let namespaced = resource
                .get("namespaced")
                .and_then(Value::as_bool)
                .ok_or(Error::InvalidResponse)?;
            if !seen.insert(name) {
                return Err(Error::InvalidResponse);
            }
            Ok(DiscoveredResource {
                name: name.to_owned(),
                namespaced,
            })
        })
        .collect()
}

fn mutation_arguments(command: &ExecCommand) -> (Vec<String>, String, String, String, bool) {
    match command {
        ExecCommand::WorkloadRestart {
            cluster: _,
            kind,
            namespace,
            name,
            dry_run,
        } => {
            let args = if *dry_run {
                let restarted_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
                let mut args =
                    base_mutation("patch", &format!("{}/{}", kind.argument(), name), true);
                args.extend([
                    format!("--namespace={namespace}"),
                    "--type=merge".into(),
                    format!(
                        "--patch={{\"spec\":{{\"template\":{{\"metadata\":{{\"annotations\":{{\"kubectl.kubernetes.io/restartedAt\":\"{restarted_at}\"}}}}}}}}}}"
                    ),
                ]);
                args
            } else {
                let mut args = base_mutation("rollout", "restart", false);
                args.extend([
                    format!("{}/{}", kind.argument(), name),
                    format!("--namespace={namespace}"),
                ]);
                args
            };
            (
                args,
                "workload_restart".into(),
                namespace.clone(),
                name.clone(),
                *dry_run,
            )
        }
        ExecCommand::WorkloadScale {
            cluster: _,
            kind,
            namespace,
            name,
            replicas,
            dry_run,
        } => {
            let mut args =
                base_mutation("scale", &format!("{}/{}", kind.argument(), name), *dry_run);
            args.extend([
                format!("--namespace={namespace}"),
                format!("--replicas={replicas}"),
            ]);
            (
                args,
                "workload_scale".into(),
                namespace.clone(),
                name.clone(),
                *dry_run,
            )
        }
        ExecCommand::CronjobSuspend {
            cluster: _,
            namespace,
            name,
            suspended,
            dry_run,
        } => {
            let mut args = base_mutation("patch", &format!("cronjob/{name}"), *dry_run);
            args.extend([
                format!("--namespace={namespace}"),
                "--type=merge".into(),
                format!("--patch={{\"spec\":{{\"suspend\":{suspended}}}}}"),
            ]);
            (
                args,
                "cronjob_suspend".into(),
                namespace.clone(),
                name.clone(),
                *dry_run,
            )
        }
        ExecCommand::CronjobTrigger {
            cluster: _,
            namespace,
            name,
            dry_run,
        } => {
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                % 1_000_000_000;
            let sequence = TRIGGER_SEQUENCE.fetch_add(1, Ordering::Relaxed) % 10_000;
            let suffix = format!("-manual-{suffix:09}-{sequence:04}");
            let prefix_len = 63 - suffix.len();
            let prefix = name[..name.len().min(prefix_len)].trim_end_matches(['-', '.']);
            let generated = format!("{prefix}{suffix}");
            let mut args = base_mutation("create", "job", *dry_run);
            args.extend([
                generated.clone(),
                format!("--from=cronjob/{name}"),
                format!("--namespace={namespace}"),
            ]);
            (
                args,
                "cronjob_trigger".into(),
                namespace.clone(),
                generated,
                *dry_run,
            )
        }
        ExecCommand::PodDelete {
            cluster: _,
            namespace,
            name,
            dry_run,
        } => {
            let mut args = base_mutation("delete", &format!("pod/{name}"), *dry_run);
            args.extend([format!("--namespace={namespace}"), "--wait=false".into()]);
            (
                args,
                "pod_delete".into(),
                namespace.clone(),
                name.clone(),
                *dry_run,
            )
        }
    }
}

fn base_mutation(first: &str, second: &str, dry_run: bool) -> Vec<String> {
    let mut args = vec![first.to_owned(), second.to_owned()];
    if dry_run {
        args.push("--dry-run=server".into());
    }
    args.push("--output=name".into());
    args
}

#[cfg(test)]
mod tests {
    use super::super::actions::{
        CapabilitiesInput, ResourceQueryInput, RestartWorkloadKind, ScalableWorkloadKind,
        valid_object_name,
    };
    use super::*;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn client(body: &str) -> (KubernetesClient, PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "homelab-kube-client-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).unwrap();
        let mut config = KubernetesConfig::new(
            path.clone(),
            PathBuf::from("/tmp/kubeconfig"),
            PathBuf::from("/tmp/kubectl-cache"),
            "context".into(),
            "cluster".into(),
        );
        config.deadline = Duration::from_secs(2);
        (KubernetesClient::new(config).unwrap(), path)
    }

    fn query(limit: u16) -> ResourceQuery {
        ResourceQueryInput {
            kind: ResourceKind::Pod,
            namespace: Some("ns".into()),
            name: None,
            labels: BTreeMap::new(),
            limit: Some(limit),
        }
        .validate()
        .unwrap()
    }

    #[tokio::test]
    async fn malformed_json_is_rejected() {
        let (client, path) = client("printf 'not-json'");
        assert_eq!(
            client.query(&query(10)).await.unwrap_err(),
            Error::InvalidResponse
        );

        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn exact_get_rejects_mismatched_identity() {
        let (client, path) = client(
            r#"printf '{"metadata":{"name":"other","namespace":"ns"},"status":{"phase":"Running"}}'"#,
        );
        let query = ResourceQueryInput {
            kind: ResourceKind::Pod,
            namespace: Some("ns".into()),
            name: Some("expected".into()),
            labels: BTreeMap::new(),
            limit: Some(1),
        }
        .validate()
        .unwrap();
        assert_eq!(
            client.query(&query).await.unwrap_err(),
            Error::InvalidResponse
        );
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn capabilities_require_exact_resource_and_scope() {
        let (client, path) = client(
            r#"printf '{"groupVersion":"v1","resources":[{"name":"pods","namespaced":true},{"name":"nodes","namespaced":false},{"name":"services","namespaced":false}]}'"#,
        );
        let query = CapabilitiesInput {
            kinds: Some(vec![
                ResourceKind::Pod,
                ResourceKind::Node,
                ResourceKind::Namespace,
                ResourceKind::Service,
            ]),
        }
        .validate()
        .unwrap();
        let QueryResult::Capabilities(result) = client.capabilities(&query).await.unwrap() else {
            panic!()
        };
        assert_eq!(result.metadata.pages, 1);
        assert_eq!(result.metadata.inspected, 4);
        assert_eq!(result.metadata.returned, 4);
        assert!(!result.metadata.truncated);
        assert!(result.capabilities[0].supported);
        assert!(result.capabilities[1].supported);
        assert!(!result.capabilities[2].supported);
        assert!(!result.capabilities[3].supported);
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn capabilities_reject_malformed_success_and_mark_fixed_unsupported_endpoint() {
        let query = CapabilitiesInput {
            kinds: Some(vec![ResourceKind::Pod]),
        }
        .validate()
        .unwrap();
        let (malformed_client, path) =
            client(r#"printf '{"groupVersion":"v1","resources":[{"name":"pods"}]}'"#);
        assert_eq!(
            malformed_client.capabilities(&query).await.unwrap_err(),
            Error::InvalidResponse
        );
        fs::remove_file(path).unwrap();

        let (client, path) =
            client("printf 'the server could not find the requested resource' >&2; exit 1");
        let QueryResult::Capabilities(result) = client.capabilities(&query).await.unwrap() else {
            panic!()
        };
        assert!(!result.capabilities[0].supported);
        assert_eq!(result.metadata.pages, 1);
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn list_limits_sort_and_report_metadata() {
        let (client, path) = client(
            r#"printf '{"items":[{"metadata":{"name":"z","namespace":"ns"},"status":{"phase":"Running"}},{"metadata":{"name":"a","namespace":"ns"},"status":{"phase":"Pending"}}],"metadata":{}}'"#,
        );
        let QueryResult::Resources(result) = client.query(&query(1)).await.unwrap() else {
            panic!()
        };
        assert_eq!(result.items[0].name, "a");
        assert_eq!(result.metadata.inspected, 2);
        assert_eq!(result.metadata.returned, 1);
        assert!(result.metadata.result_truncated && result.metadata.truncated);
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn event_messages_have_a_result_wide_byte_budget() {
        let items = (0..40)
            .map(|index| {
                serde_json::json!({
                    "metadata":{"name":format!("event-{index:02}"),"namespace":"ns"},
                    "note":"x".repeat(1_024)
                })
            })
            .collect::<Vec<_>>();
        let body =
            serde_json::to_string(&serde_json::json!({"items":items,"metadata":{}})).unwrap();
        let (client, path) = client(&format!("printf '%s' '{body}'"));
        let query = ResourceQueryInput {
            kind: ResourceKind::Event,
            namespace: Some("ns".into()),
            name: None,
            labels: BTreeMap::new(),
            limit: Some(100),
        }
        .validate()
        .unwrap();
        let QueryResult::Resources(result) = client.query(&query).await.unwrap() else {
            panic!()
        };
        assert_eq!(result.items.len(), 40);
        assert_eq!(
            result
                .items
                .iter()
                .filter_map(|event| event.details.get("message"))
                .map(String::len)
                .sum::<usize>(),
            32 * 1_024
        );
        assert!(result.items.iter().all(|event| {
            event
                .details
                .get("message")
                .is_none_or(|message| message.len() <= 1_024)
        }));
        assert!(result.metadata.aggregate_truncated && result.metadata.truncated);
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn pagination_stops_at_five_pages_and_five_hundred_items() {
        let item = r#"{"metadata":{"name":"pod","namespace":"ns"}}"#;
        let items = std::iter::repeat_n(item, 100).collect::<Vec<_>>().join(",");
        let body = format!(
            r#"case "$*" in
            *continue=next4*) next=next5 ;;
            *continue=next3*) next=next4 ;;
            *continue=next2*) next=next3 ;;
            *continue=next1*) next=next2 ;;
            *) next=next1 ;;
        esac
        printf '%s' '{{"items":[{items}],"metadata":{{"continue":"'"$next"'"}}}}'"#
        );
        let (client, path) = client(&body);
        let result = client.list(&query(100)).await.unwrap();
        assert_eq!(result.metadata.pages, 5);
        assert_eq!(result.metadata.inspected, 500);
        assert!(result.metadata.source_truncated);
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn repeated_continuation_is_rejected() {
        let (client, path) = client(r#"printf '{"items":[],"metadata":{"continue":"same"}}'"#);
        assert_eq!(
            client.list(&query(100)).await.unwrap_err(),
            Error::InvalidResponse
        );
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn selectors_and_continuations_are_url_encoded() {
        let body = r#"case "$*" in
            *continue=a%26b%3D%20c*) printf '{"items":[],"metadata":{}}' ;;
            *labelSelector=app%2Ekubernetes%2Eio%2Fname%3Dapi%2Dserver*) printf '{"items":[],"metadata":{"continue":"a&b= c"}}' ;;
            *) exit 2 ;;
        esac"#;
        let (client, path) = client(body);
        let query = ResourceQueryInput {
            kind: ResourceKind::Pod,
            namespace: Some("ns".into()),
            name: None,
            labels: [("app.kubernetes.io/name".into(), "api-server".into())].into(),
            limit: Some(10),
        }
        .validate()
        .unwrap();
        let result = client.list(&query).await.unwrap();
        assert_eq!(result.metadata.pages, 2);
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn malformed_continuation_is_rejected() {
        let (malformed_client, path) = client(r#"printf '{"items":[],"metadata":{"continue":7}}'"#);
        assert_eq!(
            malformed_client.list(&query(100)).await.unwrap_err(),
            Error::InvalidResponse
        );
        fs::remove_file(path).unwrap();

        let (missing_metadata_client, path) = client(r#"printf '{"items":[]}'"#);
        assert_eq!(
            missing_metadata_client.list(&query(100)).await.unwrap_err(),
            Error::InvalidResponse
        );
        fs::remove_file(path).unwrap();

        let oversized = "x".repeat(4_097);
        let body = serde_json::to_string(&serde_json::json!({
            "items": [],
            "metadata": {"continue": oversized}
        }))
        .unwrap();
        let (oversized_client, path) = client(&format!("printf '%s' '{body}'"));
        assert_eq!(
            oversized_client.list(&query(100)).await.unwrap_err(),
            Error::InvalidResponse
        );
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn oversized_api_page_is_rejected() {
        let item = r#"{"metadata":{"name":"pod","namespace":"ns"}}"#;
        let items = std::iter::repeat_n(item, 101).collect::<Vec<_>>().join(",");
        let (client, path) = client(&format!(
            "printf '%s' '{{\"items\":[{items}],\"metadata\":{{}}}}'"
        ));
        assert_eq!(
            client.list(&query(100)).await.unwrap_err(),
            Error::InvalidResponse
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn mutation_argv_targets_exact_objects() {
        let scale = ExecCommand::WorkloadScale {
            cluster: "cluster".into(),
            kind: ScalableWorkloadKind::Deployment,
            namespace: "ns".into(),
            name: "app".into(),
            replicas: 3,
            dry_run: true,
        };
        let (args, _, _, _, _) = mutation_arguments(&scale);
        assert_eq!(
            args,
            [
                "scale",
                "deployment/app",
                "--dry-run=server",
                "--output=name",
                "--namespace=ns",
                "--replicas=3"
            ]
        );
        assert!(
            !args
                .iter()
                .any(|arg| arg == "--all" || arg.contains("selector"))
        );

        let restart = ExecCommand::WorkloadRestart {
            cluster: "cluster".into(),
            kind: RestartWorkloadKind::DaemonSet,
            namespace: "system".into(),
            name: "agent".into(),
            dry_run: false,
        };
        assert_eq!(
            mutation_arguments(&restart).0,
            [
                "rollout",
                "restart",
                "--output=name",
                "daemonset/agent",
                "--namespace=system"
            ]
        );
        let dry_restart = ExecCommand::WorkloadRestart {
            cluster: "cluster".into(),
            kind: RestartWorkloadKind::DaemonSet,
            namespace: "system".into(),
            name: "agent".into(),
            dry_run: true,
        };
        let restart_args = mutation_arguments(&dry_restart).0;
        assert_eq!(
            &restart_args[..6],
            [
                "patch",
                "daemonset/agent",
                "--dry-run=server",
                "--output=name",
                "--namespace=system",
                "--type=merge"
            ]
        );
        assert!(restart_args[6].starts_with(
            "--patch={\"spec\":{\"template\":{\"metadata\":{\"annotations\":{\"kubectl.kubernetes.io/restartedAt\":\""
        ));

        let suspend = ExecCommand::CronjobSuspend {
            cluster: "cluster".into(),
            namespace: "jobs".into(),
            name: "backup".into(),
            suspended: true,
            dry_run: true,
        };
        assert_eq!(
            mutation_arguments(&suspend).0,
            [
                "patch",
                "cronjob/backup",
                "--dry-run=server",
                "--output=name",
                "--namespace=jobs",
                "--type=merge",
                "--patch={\"spec\":{\"suspend\":true}}"
            ]
        );

        let trigger = ExecCommand::CronjobTrigger {
            cluster: "cluster".into(),
            namespace: "jobs".into(),
            name: "backup".into(),
            dry_run: true,
        };
        let trigger_args = mutation_arguments(&trigger).0;
        assert_eq!(&trigger_args[..2], ["create", "job"]);
        assert_eq!(trigger_args[2], "--dry-run=server");
        assert_eq!(trigger_args[3], "--output=name");
        assert!(trigger_args[4].starts_with("backup-manual-"));
        assert_eq!(trigger_args[5], "--from=cronjob/backup");
        assert_eq!(trigger_args[6], "--namespace=jobs");

        let long_trigger = ExecCommand::CronjobTrigger {
            cluster: "cluster".into(),
            namespace: "jobs".into(),
            name: "a".repeat(52),
            dry_run: false,
        };
        let generated = mutation_arguments(&long_trigger).3;
        assert!(generated.len() <= 63 && valid_object_name(&generated));

        let delete = ExecCommand::PodDelete {
            cluster: "cluster".into(),
            namespace: "apps".into(),
            name: "pod-1".into(),
            dry_run: false,
        };
        assert_eq!(
            mutation_arguments(&delete).0,
            [
                "delete",
                "pod/pod-1",
                "--output=name",
                "--namespace=apps",
                "--wait=false"
            ]
        );
        let dry_delete = ExecCommand::PodDelete {
            cluster: "cluster".into(),
            namespace: "apps".into(),
            name: "pod-1".into(),
            dry_run: true,
        };
        assert_eq!(
            mutation_arguments(&dry_delete).0,
            [
                "delete",
                "pod/pod-1",
                "--dry-run=server",
                "--output=name",
                "--namespace=apps",
                "--wait=false"
            ]
        );
    }
}
