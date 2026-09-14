use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use super::{
    Error,
    actions::{ResourceKind, valid_namespace, valid_object_name},
};

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ListMetadata {
    pub pages: u8,
    pub inspected: u16,
    pub returned: u16,
    pub source_truncated: bool,
    pub result_truncated: bool,
    pub aggregate_truncated: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct Condition {
    pub condition_type: String,
    pub status: String,
    pub reason: Option<String>,
    pub last_transition_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct Resource {
    pub kind: ResourceKind,
    pub api_version: String,
    pub namespace: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    pub created_at: Option<String>,
    pub status: Option<String>,
    pub details: BTreeMap<String, String>,
    pub details_truncated: bool,
    pub conditions: Vec<Condition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_statuses: Option<ContainerStatuses>,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContainerKind {
    Init,
    Application,
    Ephemeral,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ContainerStatus {
    pub kind: ContainerKind,
    pub name: String,
    pub ready: Option<bool>,
    pub started: Option<bool>,
    pub restart_count: u64,
    pub state: String,
    pub reason: Option<String>,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub last_reason: Option<String>,
    pub last_exit_code: Option<i32>,
    pub last_signal: Option<i32>,
    pub last_started_at: Option<String>,
    pub last_finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ContainerStatuses {
    pub count: u16,
    pub returned: u16,
    pub truncated: bool,
    pub items: Vec<ContainerStatus>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ResourceList {
    pub kind: ResourceKind,
    pub items: Vec<Resource>,
    pub metadata: ListMetadata,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct Cluster {
    pub name: String,
    pub context: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ClusterCatalog {
    pub clusters: Vec<Cluster>,
    pub metadata: ListMetadata,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct Capability {
    pub kind: ResourceKind,
    pub api_version: String,
    pub supported: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct CapabilityCatalog {
    pub capabilities: Vec<Capability>,
    pub metadata: ListMetadata,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "type", content = "result", rename_all = "snake_case")]
pub enum QueryResult {
    Clusters(ClusterCatalog),
    Capabilities(CapabilityCatalog),
    Resources(ResourceList),
    Resource(Resource),
    PodLogs(PodLogs),
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct PodLogs {
    pub cluster: String,
    pub namespace: String,
    pub pod: String,
    pub pod_uid: String,
    pub container: String,
    pub instance: String,
    pub timestamps: bool,
    pub tail_lines: u16,
    pub max_bytes: u32,
    pub text: String,
    pub line_count: u16,
    pub tail_truncated: bool,
    pub byte_truncated: bool,
    pub redacted: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ExecResult {
    pub status: String,
    pub action: String,
    pub namespace: String,
    pub name: String,
    pub dry_run: bool,
}

pub(crate) fn resource(kind: ResourceKind, value: &Value) -> Result<Resource, Error> {
    let metadata = value.get("metadata").ok_or(Error::InvalidResponse)?;
    let name = metadata
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| valid_object_name(name))
        .ok_or(Error::InvalidResponse)?
        .to_owned();
    let namespace = metadata.get("namespace").and_then(Value::as_str);
    let namespace = if kind.namespaced() {
        Some(
            namespace
                .filter(|namespace| valid_namespace(namespace))
                .ok_or(Error::InvalidResponse)?
                .to_owned(),
        )
    } else if namespace.is_some() {
        return Err(Error::InvalidResponse);
    } else {
        None
    };
    let conditions = normalize_conditions(value);
    let status = status_for(kind, value);
    let uid = if kind == ResourceKind::Pod {
        Some(
            token_text(metadata.get("uid"), 128)
                .ok_or(Error::InvalidResponse)?
                .to_owned(),
        )
    } else {
        None
    };
    let container_statuses =
        (kind == ResourceKind::Pod).then(|| normalize_container_statuses(value));
    let mut details = BTreeMap::new();
    let mut details_truncated = false;
    match kind {
        ResourceKind::Deployment | ResourceKind::StatefulSet | ResourceKind::ReplicaSet => {
            add_number(
                &mut details,
                "desired_replicas",
                value.pointer("/spec/replicas"),
            );
            add_number(
                &mut details,
                "ready_replicas",
                value.pointer("/status/readyReplicas"),
            );
            add_number(
                &mut details,
                "available_replicas",
                value.pointer("/status/availableReplicas"),
            );
        }
        ResourceKind::DaemonSet => {
            add_number(
                &mut details,
                "desired_nodes",
                value.pointer("/status/desiredNumberScheduled"),
            );
            add_number(
                &mut details,
                "ready_nodes",
                value.pointer("/status/numberReady"),
            );
        }
        ResourceKind::Job => {
            add_number(&mut details, "active", value.pointer("/status/active"));
            add_number(
                &mut details,
                "succeeded",
                value.pointer("/status/succeeded"),
            );
            add_number(&mut details, "failed", value.pointer("/status/failed"));
        }
        ResourceKind::CronJob => {
            add_bool(&mut details, "suspended", value.pointer("/spec/suspend"));
            add_text(
                &mut details,
                "schedule",
                value.pointer("/spec/schedule"),
                256,
            );
            add_token(
                &mut details,
                "last_schedule_time",
                value.pointer("/status/lastScheduleTime"),
                64,
            );
        }
        ResourceKind::Pod => {
            add_token(&mut details, "node", value.pointer("/spec/nodeName"), 253);
            add_token(&mut details, "pod_ip", value.pointer("/status/podIP"), 64);
            add_number(
                &mut details,
                "restarts",
                Some(&Value::from(restart_count(value))),
            );
        }
        ResourceKind::Event => {
            add_token(&mut details, "reason", value.get("reason"), 128);
            details_truncated = add_truncated_text(
                &mut details,
                "message",
                value.get("note").or_else(|| value.get("message")),
                1_024,
            );
            add_token(
                &mut details,
                "regarding_kind",
                value.pointer("/regarding/kind"),
                128,
            );
            add_token(
                &mut details,
                "regarding_name",
                value.pointer("/regarding/name"),
                253,
            );
            add_number(&mut details, "count", value.pointer("/deprecatedCount"));
        }
        ResourceKind::PodMetric => {
            normalize_pod_metric(&mut details, value);
            add_token(&mut details, "timestamp", value.get("timestamp"), 64);
            add_token(&mut details, "window", value.get("window"), 64);
        }
        ResourceKind::NodeMetric => {
            let usage = value.pointer("/usage");
            add_quantity(&mut details, "cpu", usage.and_then(|v| v.get("cpu")));
            add_quantity(&mut details, "memory", usage.and_then(|v| v.get("memory")));
            add_token(&mut details, "timestamp", value.get("timestamp"), 64);
            add_token(&mut details, "window", value.get("window"), 64);
        }
        ResourceKind::Service => {
            add_token(&mut details, "type", value.pointer("/spec/type"), 64);
            add_token(
                &mut details,
                "cluster_ip",
                value.pointer("/spec/clusterIP"),
                64,
            );
        }
        ResourceKind::EndpointSlice => {
            let endpoints = value
                .get("endpoints")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let ready = endpoints
                .iter()
                .filter(|endpoint| match endpoint.pointer("/conditions/ready") {
                    None | Some(Value::Null) => true,
                    Some(Value::Bool(ready)) => *ready,
                    Some(_) => false,
                })
                .count();
            details.insert("endpoints".into(), endpoints.len().to_string());
            details.insert("ready_endpoints".into(), ready.to_string());
        }
        ResourceKind::Ingress => add_token(
            &mut details,
            "class",
            value.pointer("/spec/ingressClassName"),
            253,
        ),
        ResourceKind::Gateway => add_token(
            &mut details,
            "class",
            value.pointer("/spec/gatewayClassName"),
            253,
        ),
        ResourceKind::GatewayClass => add_token(
            &mut details,
            "controller",
            value.pointer("/spec/controllerName"),
            253,
        ),
        ResourceKind::HttpRoute => add_number(
            &mut details,
            "rules",
            value
                .pointer("/spec/rules")
                .and_then(|v| v.as_array().map(|a| Value::from(a.len())))
                .as_ref(),
        ),
        ResourceKind::PersistentVolumeClaim => {
            add_token(
                &mut details,
                "storage_class",
                value.pointer("/spec/storageClassName"),
                253,
            );
            add_quantity(
                &mut details,
                "capacity",
                value.pointer("/status/capacity/storage"),
            );
        }
        ResourceKind::StorageClass => {
            add_token(&mut details, "provisioner", value.get("provisioner"), 253);
            add_token(
                &mut details,
                "reclaim_policy",
                value.get("reclaimPolicy"),
                64,
            );
        }
        ResourceKind::HorizontalPodAutoscaler => {
            add_number(
                &mut details,
                "current_replicas",
                value.pointer("/status/currentReplicas"),
            );
            add_number(
                &mut details,
                "desired_replicas",
                value.pointer("/status/desiredReplicas"),
            );
        }
        ResourceKind::PodDisruptionBudget => add_number(
            &mut details,
            "disruptions_allowed",
            value.pointer("/status/disruptionsAllowed"),
        ),
        ResourceKind::NetworkPolicy => {
            add_number(
                &mut details,
                "ingress_rules",
                value
                    .pointer("/spec/ingress")
                    .and_then(|v| v.as_array().map(|a| Value::from(a.len())))
                    .as_ref(),
            );
            add_number(
                &mut details,
                "egress_rules",
                value
                    .pointer("/spec/egress")
                    .and_then(|v| v.as_array().map(|a| Value::from(a.len())))
                    .as_ref(),
            );
        }
        ResourceKind::VeleroBackup => {
            add_token(&mut details, "phase", value.pointer("/status/phase"), 64)
        }
        ResourceKind::VeleroSchedule => add_text(
            &mut details,
            "schedule",
            value.pointer("/spec/schedule"),
            256,
        ),
        ResourceKind::BackupStorageLocation => {
            add_token(
                &mut details,
                "provider",
                value.pointer("/spec/provider"),
                128,
            );
            add_token(&mut details, "phase", value.pointer("/status/phase"), 64);
        }
        _ => {}
    }
    Ok(Resource {
        kind,
        api_version: kind.api_version(),
        namespace,
        name,
        uid,
        created_at: token_text(metadata.get("creationTimestamp"), 64).map(str::to_owned),
        status,
        details,
        details_truncated,
        conditions,
        container_statuses,
    })
}

fn normalize_container_statuses(value: &Value) -> ContainerStatuses {
    const MAX_CONTAINER_STATUSES: usize = 32;
    let groups = [
        (ContainerKind::Init, "/status/initContainerStatuses"),
        (ContainerKind::Application, "/status/containerStatuses"),
        (
            ContainerKind::Ephemeral,
            "/status/ephemeralContainerStatuses",
        ),
    ];
    let count = groups
        .iter()
        .map(|(_, pointer)| {
            value
                .pointer(pointer)
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        })
        .sum::<usize>();
    let items = groups
        .into_iter()
        .flat_map(|(kind, pointer)| {
            value
                .pointer(pointer)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(move |status| (kind, status))
        })
        .take(MAX_CONTAINER_STATUSES)
        .filter_map(|(kind, status)| normalize_container_status(kind, status))
        .collect::<Vec<_>>();
    ContainerStatuses {
        count: count.min(usize::from(u16::MAX)) as u16,
        returned: items.len() as u16,
        truncated: count > MAX_CONTAINER_STATUSES,
        items,
    }
}

fn normalize_container_status(kind: ContainerKind, value: &Value) -> Option<ContainerStatus> {
    let name = token_text(value.get("name"), 253)?.to_owned();
    let current = value.get("state");
    let (state, state_value) = if let Some(running) = current.and_then(|v| v.get("running")) {
        ("running", Some(running))
    } else if let Some(waiting) = current.and_then(|v| v.get("waiting")) {
        ("waiting", Some(waiting))
    } else if let Some(terminated) = current.and_then(|v| v.get("terminated")) {
        ("terminated", Some(terminated))
    } else {
        ("unknown", None)
    };
    let last = value.pointer("/lastState/terminated");
    Some(ContainerStatus {
        kind,
        name,
        ready: value.get("ready").and_then(Value::as_bool),
        started: value.get("started").and_then(Value::as_bool),
        restart_count: value
            .get("restartCount")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        state: state.to_owned(),
        reason: token_text(state_value.and_then(|v| v.get("reason")), 128).map(str::to_owned),
        exit_code: signed_i32(state_value.and_then(|v| v.get("exitCode"))),
        signal: signed_i32(state_value.and_then(|v| v.get("signal"))),
        started_at: token_text(state_value.and_then(|v| v.get("startedAt")), 64).map(str::to_owned),
        finished_at: token_text(state_value.and_then(|v| v.get("finishedAt")), 64)
            .map(str::to_owned),
        last_reason: token_text(last.and_then(|v| v.get("reason")), 128).map(str::to_owned),
        last_exit_code: signed_i32(last.and_then(|v| v.get("exitCode"))),
        last_signal: signed_i32(last.and_then(|v| v.get("signal"))),
        last_started_at: token_text(last.and_then(|v| v.get("startedAt")), 64).map(str::to_owned),
        last_finished_at: token_text(last.and_then(|v| v.get("finishedAt")), 64).map(str::to_owned),
    })
}

fn signed_i32(value: Option<&Value>) -> Option<i32> {
    value?.as_i64()?.try_into().ok()
}

fn normalize_conditions(value: &Value) -> Vec<Condition> {
    value
        .pointer("/status/conditions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(20)
        .filter_map(|item| {
            Some(Condition {
                condition_type: token_text(item.get("type"), 128)?.to_owned(),
                status: token_text(item.get("status"), 32)?.to_owned(),
                reason: token_text(item.get("reason"), 128).map(str::to_owned),
                last_transition_time: token_text(item.get("lastTransitionTime"), 64)
                    .map(str::to_owned),
            })
        })
        .collect()
}

fn status_for(kind: ResourceKind, value: &Value) -> Option<String> {
    let pointer = match kind {
        ResourceKind::Pod => "/status/phase",
        ResourceKind::PersistentVolumeClaim => "/status/phase",
        ResourceKind::Certificate => "/status/conditions/0/status",
        ResourceKind::CnpgCluster => "/status/phase",
        ResourceKind::Kafka | ResourceKind::KafkaNodePool | ResourceKind::KafkaTopic => {
            "/status/conditions/0/type"
        }
        ResourceKind::CephCluster => "/status/phase",
        ResourceKind::CephFilesystem
        | ResourceKind::CephBlockPool
        | ResourceKind::CephObjectStore => "/status/phase",
        ResourceKind::VeleroBackup | ResourceKind::BackupStorageLocation => "/status/phase",
        _ => return None,
    };
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .filter(|v| token(v, 128))
        .map(str::to_owned)
}

fn restart_count(value: &Value) -> u64 {
    [
        "/status/initContainerStatuses",
        "/status/containerStatuses",
        "/status/ephemeralContainerStatuses",
    ]
    .into_iter()
    .filter_map(|pointer| value.pointer(pointer).and_then(Value::as_array))
    .flatten()
    .filter_map(|v| v.get("restartCount").and_then(Value::as_u64))
    .fold(0_u64, u64::saturating_add)
}

fn normalize_pod_metric(details: &mut BTreeMap<String, String>, value: &Value) {
    const MAX_CONTAINERS: usize = 20;
    let containers = value
        .get("containers")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut returned = 0usize;
    for container in containers.iter().take(MAX_CONTAINERS) {
        let Some(name) = container
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| valid_metric_name(name))
        else {
            continue;
        };
        let index = returned;
        details.insert(format!("container.{index}.name"), name.to_owned());
        add_quantity(
            details,
            &format!("container.{index}.cpu"),
            container.pointer("/usage/cpu"),
        );
        add_quantity(
            details,
            &format!("container.{index}.memory"),
            container.pointer("/usage/memory"),
        );
        returned += 1;
    }
    details.insert("container_count".into(), containers.len().to_string());
    details.insert("containers_returned".into(), returned.to_string());
    details.insert(
        "containers_truncated".into(),
        (containers.len() > MAX_CONTAINERS).to_string(),
    );
}

fn valid_metric_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && !value.starts_with('-')
        && value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

fn add_quantity(out: &mut BTreeMap<String, String>, key: &str, value: Option<&Value>) {
    if let Some(value) = value
        .and_then(Value::as_str)
        .filter(|value| quantity(value))
    {
        out.insert(key.to_owned(), value.to_owned());
    }
}

fn quantity(value: &str) -> bool {
    if value.is_empty() || value.len() > 64 || value.chars().any(char::is_control) {
        return false;
    }
    if value.parse::<f64>().is_ok_and(|number| number.is_finite()) {
        return true;
    }
    let suffix_at = value
        .char_indices()
        .find_map(|(index, character)| character.is_ascii_alphabetic().then_some(index))
        .unwrap_or(value.len());
    let (number, suffix) = value.split_at(suffix_at);
    number.parse::<f64>().is_ok_and(|number| number.is_finite())
        && matches!(
            suffix,
            "" | "n"
                | "u"
                | "m"
                | "k"
                | "K"
                | "M"
                | "G"
                | "T"
                | "P"
                | "E"
                | "Ki"
                | "Mi"
                | "Gi"
                | "Ti"
                | "Pi"
                | "Ei"
        )
}

fn safe(value: &str, max: usize) -> bool {
    value.len() <= max && !value.chars().any(char::is_control)
}
fn add_text(out: &mut BTreeMap<String, String>, key: &str, value: Option<&Value>, max: usize) {
    if let Some(v) = value.and_then(Value::as_str).filter(|v| safe(v, max)) {
        out.insert(key.into(), v.into());
    }
}

fn add_truncated_text(
    out: &mut BTreeMap<String, String>,
    key: &str,
    value: Option<&Value>,
    max: usize,
) -> bool {
    if let Some(value) = value.and_then(Value::as_str) {
        let truncated = truncate_utf8(value, max);
        if !truncated.chars().any(char::is_control) {
            out.insert(key.into(), truncated.to_owned());
            return truncated.len() < value.len();
        }
    }
    false
}

fn add_token(out: &mut BTreeMap<String, String>, key: &str, value: Option<&Value>, max: usize) {
    if let Some(value) = token_text(value, max) {
        out.insert(key.into(), value.into());
    }
}

fn token_text(value: Option<&Value>, max: usize) -> Option<&str> {
    value?.as_str().filter(|value| token(value, max))
}

fn token(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '.' | '_' | '/' | ':')
        })
}

fn truncate_utf8(value: &str, max: usize) -> &str {
    if value.len() <= max {
        return value;
    }
    let mut boundary = max;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &value[..boundary]
}

pub(crate) fn bound_event_messages(resources: &mut [Resource]) -> bool {
    let mut remaining = 32 * 1_024;
    let mut aggregate_truncated = false;
    for resource in resources {
        if remaining == 0 && resource.details.contains_key("message") {
            resource.details.remove("message");
            resource.details_truncated = true;
            aggregate_truncated = true;
            continue;
        }
        if let Some(message) = resource.details.get_mut("message") {
            let retained = truncate_utf8(message, remaining).len();
            if retained < message.len() {
                resource.details_truncated = true;
                aggregate_truncated = true;
            }
            message.truncate(retained);
            remaining -= retained;
        }
    }
    aggregate_truncated
}
fn add_number(out: &mut BTreeMap<String, String>, key: &str, value: Option<&Value>) {
    if let Some(v) = value.and_then(Value::as_u64) {
        out.insert(key.into(), v.to_string());
    }
}
fn add_bool(out: &mut BTreeMap<String, String>, key: &str, value: Option<&Value>) {
    if let Some(v) = value.and_then(Value::as_bool) {
        out.insert(key.into(), v.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn arbitrary_metadata_and_unreviewed_nested_content_are_omitted() {
        let result = resource(ResourceKind::Pod, &json!({
            "metadata":{"name":"pod","namespace":"ns","uid":"pod-uid","labels":{"app":"SENTINEL"},"annotations":{"note":"SENTINEL"}},
            "spec":{"token":"opaque-SENTINEL-742"}, "data":{"key":"opaque-SENTINEL-742"},
            "status":{"phase":"Running","conditions":[{"type":"Ready","status":"False","reason":"Invalid","message":"opaque-SENTINEL-742"}]}
        })).unwrap();
        let encoded = serde_json::to_string(&result).unwrap();
        assert!(!encoded.contains("SENTINEL"));
        assert!(!encoded.contains("labels"));
        assert!(!encoded.contains("annotations"));
    }

    #[test]
    fn pod_container_lifecycle_statuses_are_safe_ordered_and_complete() {
        let result = resource(ResourceKind::Pod, &json!({
            "metadata":{"name":"pod","namespace":"ns","uid":"pod-uid"},
            "spec":{
                "containers":[{
                    "name":"app","image":"private/image","args":["SENTINEL"],
                    "env":[{"name":"TOKEN","valueFrom":{"secretKeyRef":{"name":"SENTINEL"}}}],
                    "volumeMounts":[{"name":"SENTINEL","mountPath":"/secret"}]
                }],
                "volumes":[{"name":"SENTINEL","secret":{"secretName":"SENTINEL"}}]
            },
            "status":{
                "initContainerStatuses":[{
                    "name":"init","ready":true,"started":true,"restartCount":1,
                    "state":{"running":{"startedAt":"2026-01-01T00:00:00Z"}},
                    "lastState":{"terminated":{"reason":"Completed","exitCode":0,"signal":0,"startedAt":"2025-12-31T23:00:00Z","finishedAt":"2025-12-31T23:01:00Z","message":"SENTINEL"}},
                    "image":"SENTINEL","imageID":"SENTINEL","containerID":"SENTINEL"
                }],
                "containerStatuses":[
                    {"name":"app","ready":false,"started":false,"restartCount":2,"state":{"waiting":{"reason":"CrashLoopBackOff","message":"SENTINEL"}}},
                    {"name":"done","restartCount":3,"state":{"terminated":{"reason":"Error","exitCode":17,"signal":9,"startedAt":"2026-01-01T01:00:00Z","finishedAt":"2026-01-01T01:01:00Z","message":"SENTINEL"}}},
                    {"name":"unknown","restartCount":4,"state":{}}
                ],
                "ephemeralContainerStatuses":[{"name":"debugger","restartCount":5,"state":{"running":{}}}]
            }
        })).unwrap();
        assert_eq!(result.uid.as_deref(), Some("pod-uid"));
        assert_eq!(
            result.details.get("restarts").map(String::as_str),
            Some("15")
        );
        let statuses = result.container_statuses.as_ref().unwrap();
        assert_eq!(
            (statuses.count, statuses.returned, statuses.truncated),
            (5, 5, false)
        );
        assert_eq!(
            statuses
                .items
                .iter()
                .map(|item| (item.kind, item.name.as_str(), item.state.as_str()))
                .collect::<Vec<_>>(),
            [
                (ContainerKind::Init, "init", "running"),
                (ContainerKind::Application, "app", "waiting"),
                (ContainerKind::Application, "done", "terminated"),
                (ContainerKind::Application, "unknown", "unknown"),
                (ContainerKind::Ephemeral, "debugger", "running"),
            ]
        );
        assert_eq!(statuses.items[0].last_reason.as_deref(), Some("Completed"));
        assert_eq!(statuses.items[0].last_exit_code, Some(0));
        assert_eq!(statuses.items[0].last_signal, Some(0));
        assert_eq!(
            statuses.items[0].started_at.as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
        assert_eq!(
            statuses.items[0].last_finished_at.as_deref(),
            Some("2025-12-31T23:01:00Z")
        );
        assert_eq!(statuses.items[2].exit_code, Some(17));
        assert_eq!(statuses.items[2].signal, Some(9));
        let encoded = serde_json::to_string(&result).unwrap();
        for excluded in [
            "SENTINEL",
            "imageID",
            "containerID",
            "message",
            "env",
            "args",
            "volumeMounts",
            "volumes",
            "secretKeyRef",
        ] {
            assert!(
                !encoded.contains(excluded),
                "included unsafe field {excluded}"
            );
        }
    }

    #[test]
    fn pod_container_statuses_have_a_deterministic_total_cap() {
        let statuses = |prefix: &str, count: usize| {
            (0..count)
                .map(|index| json!({"name":format!("{prefix}-{index:02}"),"restartCount":0,"state":{}}))
                .collect::<Vec<_>>()
        };
        let result = resource(
            ResourceKind::Pod,
            &json!({
                "metadata":{"name":"pod","namespace":"ns","uid":"uid"},
                "status":{
                    "initContainerStatuses":statuses("init", 16),
                    "containerStatuses":statuses("app", 16),
                    "ephemeralContainerStatuses":statuses("ephemeral", 4)
                }
            }),
        )
        .unwrap();
        let statuses = result.container_statuses.unwrap();
        assert_eq!(
            (statuses.count, statuses.returned, statuses.truncated),
            (36, 32, true)
        );
        assert_eq!(statuses.items[0].name, "init-00");
        assert_eq!(statuses.items[31].name, "app-15");
        assert!(
            statuses
                .items
                .iter()
                .all(|status| status.kind != ContainerKind::Ephemeral)
        );
    }

    #[test]
    fn pod_uid_is_required_and_bounded() {
        let oversized = "u".repeat(129);
        for uid in [None, Some("bad uid"), Some(""), Some(oversized.as_str())] {
            let mut value = json!({"metadata":{"name":"pod","namespace":"ns"}});
            if let Some(uid) = uid {
                value["metadata"]["uid"] = json!(uid);
            }
            assert_eq!(
                resource(ResourceKind::Pod, &value),
                Err(Error::InvalidResponse)
            );
        }
    }

    #[test]
    fn event_messages_and_conditions_are_bounded() {
        let conditions = (0..30)
            .map(|index| json!({"type":format!("Type{index}"),"status":"True","reason":"Valid","message":"UNREVIEWED"}))
            .collect::<Vec<_>>();
        let message = "é".repeat(600);
        let event = resource(
            ResourceKind::Event,
            &json!({
                "metadata":{"name":"event","namespace":"ns"},
                "note":message,
                "status":{"conditions":conditions}
            }),
        )
        .unwrap();
        let retained = event.details.get("message").unwrap();
        assert!(retained.len() <= 1_024 && retained.is_char_boundary(retained.len()));
        assert!(event.details_truncated);
        assert_eq!(event.conditions.len(), 20);
        assert!(
            !serde_json::to_string(&event)
                .unwrap()
                .contains("UNREVIEWED")
        );

        let mut events = (0..40)
            .map(|index| {
                resource(
                    ResourceKind::Event,
                    &json!({
                        "metadata":{"name":format!("event-{index}"),"namespace":"ns"},
                        "note":"x".repeat(1_024)
                    }),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(bound_event_messages(&mut events));
        assert_eq!(
            events
                .iter()
                .filter_map(|event| event.details.get("message"))
                .map(String::len)
                .sum::<usize>(),
            32 * 1_024
        );
        assert!(events[32].details_truncated);
    }

    #[test]
    fn endpoint_slice_absent_or_null_ready_is_ready() {
        let result = resource(ResourceKind::EndpointSlice, &json!({
            "metadata":{"name":"slice","namespace":"ns"},
            "endpoints":[{}, {"conditions":{"ready":null}}, {"conditions":{"ready":true}}, {"conditions":{"ready":false}}]
        })).unwrap();
        assert_eq!(
            result.details.get("endpoints").map(String::as_str),
            Some("4")
        );
        assert_eq!(
            result.details.get("ready_endpoints").map(String::as_str),
            Some("3")
        );
    }

    #[test]
    fn pod_metrics_are_bounded_per_container_without_false_aggregate() {
        let mut containers = vec![
            json!({"name":"api","usage":{"cpu":"10m","memory":"20Mi"}}),
            json!({"name":"worker","usage":{"cpu":"30m","memory":"40Mi"}}),
            json!({"name":"UNSAFE NAME","usage":{"cpu":"opaque","memory":"opaque"}}),
        ];
        containers.extend((0..20).map(
            |index| json!({"name":format!("extra-{index}"),"usage":{"cpu":"1m","memory":"1Mi"}}),
        ));
        let result = resource(
            ResourceKind::PodMetric,
            &json!({
                "metadata":{"name":"pod","namespace":"ns"}, "containers":containers,
                "usage":{"cpu":"WRONG-AGGREGATE","memory":"WRONG-AGGREGATE"}
            }),
        )
        .unwrap();
        assert_eq!(
            result.details.get("container.0.name").map(String::as_str),
            Some("api")
        );
        assert_eq!(
            result.details.get("container.1.cpu").map(String::as_str),
            Some("30m")
        );
        assert_eq!(
            result.details.get("container_count").map(String::as_str),
            Some("23")
        );
        assert_eq!(
            result
                .details
                .get("containers_returned")
                .map(String::as_str),
            Some("19")
        );
        assert_eq!(
            result
                .details
                .get("containers_truncated")
                .map(String::as_str),
            Some("true")
        );
        assert!(!result.details.contains_key("cpu"));
        assert!(
            !result
                .details
                .values()
                .any(|value| value.contains("UNSAFE"))
        );
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("WRONG-AGGREGATE")
        );
    }
}
