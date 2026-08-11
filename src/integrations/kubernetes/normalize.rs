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
    pub created_at: Option<String>,
    pub status: Option<String>,
    pub details: BTreeMap<String, String>,
    pub details_truncated: bool,
    pub conditions: Vec<Condition>,
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
        created_at: token_text(metadata.get("creationTimestamp"), 64).map(str::to_owned),
        status,
        details,
        details_truncated,
        conditions,
    })
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
    value
        .pointer("/status/containerStatuses")
        .and_then(Value::as_array)
        .into_iter()
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
            "metadata":{"name":"pod","namespace":"ns","labels":{"app":"SENTINEL"},"annotations":{"note":"SENTINEL"}},
            "spec":{"token":"opaque-SENTINEL-742"}, "data":{"key":"opaque-SENTINEL-742"},
            "status":{"phase":"Running","conditions":[{"type":"Ready","status":"False","reason":"Invalid","message":"opaque-SENTINEL-742"}]}
        })).unwrap();
        let encoded = serde_json::to_string(&result).unwrap();
        assert!(!encoded.contains("SENTINEL"));
        assert!(!encoded.contains("labels"));
        assert!(!encoded.contains("annotations"));
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
