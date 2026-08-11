use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const MAX_SELECTORS: usize = 8;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryInput {
    ClusterList {},
    CapabilityList {
        cluster: String,
        kinds: Option<Vec<ResourceKind>>,
    },
    ResourceList {
        cluster: String,
        kind: ResourceKind,
        namespace: Option<String>,
        #[serde(default)]
        labels: BTreeMap<String, String>,
        limit: Option<u16>,
    },
    ResourceGet {
        cluster: String,
        kind: ResourceKind,
        namespace: Option<String>,
        name: String,
    },
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClusterListInput {}

impl ClusterListInput {
    pub fn validate(self) -> Result<QueryCommand, ValidationError> {
        QueryInput::ClusterList {}.validate()
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityListInput {
    pub cluster: String,
    pub kinds: Option<Vec<ResourceKind>>,
}

impl CapabilityListInput {
    pub fn validate(self) -> Result<QueryCommand, ValidationError> {
        QueryInput::CapabilityList {
            cluster: self.cluster,
            kinds: self.kinds,
        }
        .validate()
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum ResourceListInput {
    Namespaced {
        cluster: String,
        kind: NamespacedResourceKind,
        namespace: String,
        #[serde(default)]
        labels: BTreeMap<String, String>,
        limit: Option<u16>,
    },
    Cluster {
        cluster: String,
        kind: ClusterResourceKind,
        #[serde(default)]
        labels: BTreeMap<String, String>,
        limit: Option<u16>,
    },
}

impl ResourceListInput {
    pub fn validate(self) -> Result<QueryCommand, ValidationError> {
        let (cluster, kind, namespace, labels, limit) = match self {
            Self::Namespaced {
                cluster,
                kind,
                namespace,
                labels,
                limit,
            } => (cluster, kind.into(), Some(namespace), labels, limit),
            Self::Cluster {
                cluster,
                kind,
                labels,
                limit,
            } => (cluster, kind.into(), None, labels, limit),
        };
        QueryInput::ResourceList {
            cluster,
            kind,
            namespace,
            labels,
            limit,
        }
        .validate()
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum ResourceGetInput {
    Namespaced {
        cluster: String,
        kind: NamespacedResourceKind,
        namespace: String,
        name: String,
    },
    Cluster {
        cluster: String,
        kind: ClusterResourceKind,
        name: String,
    },
}

impl ResourceGetInput {
    pub fn validate(self) -> Result<QueryCommand, ValidationError> {
        let (cluster, kind, namespace, name) = match self {
            Self::Namespaced {
                cluster,
                kind,
                namespace,
                name,
            } => (cluster, kind.into(), Some(namespace), name),
            Self::Cluster {
                cluster,
                kind,
                name,
            } => (cluster, kind.into(), None, name),
        };
        QueryInput::ResourceGet {
            cluster,
            kind,
            namespace,
            name,
        }
        .validate()
    }
}

#[derive(Debug, Clone)]
pub enum QueryCommand {
    ClusterList,
    CapabilityList {
        cluster: String,
        query: CapabilitiesQuery,
    },
    Resource {
        cluster: String,
        query: ResourceQuery,
    },
}

impl QueryInput {
    pub fn validate(self) -> Result<QueryCommand, ValidationError> {
        match self {
            Self::ClusterList {} => Ok(QueryCommand::ClusterList),
            Self::CapabilityList { cluster, kinds } if valid_catalog_name(&cluster) => {
                Ok(QueryCommand::CapabilityList {
                    cluster,
                    query: CapabilitiesInput { kinds }.validate()?,
                })
            }
            Self::ResourceList {
                cluster,
                kind,
                namespace,
                labels,
                limit,
            } if valid_catalog_name(&cluster) => Ok(QueryCommand::Resource {
                cluster,
                query: ResourceQueryInput {
                    kind,
                    namespace,
                    name: None,
                    labels,
                    limit,
                }
                .validate()?,
            }),
            Self::ResourceGet {
                cluster,
                kind,
                namespace,
                name,
            } if valid_catalog_name(&cluster) => Ok(QueryCommand::Resource {
                cluster,
                query: ResourceQueryInput {
                    kind,
                    namespace,
                    name: Some(name),
                    labels: BTreeMap::new(),
                    limit: Some(1),
                }
                .validate()?,
            }),
            _ => Err(ValidationError),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidationError;

impl std::fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("invalid Kubernetes arguments")
    }
}

impl std::error::Error for ValidationError {}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Namespace,
    Node,
    Deployment,
    StatefulSet,
    DaemonSet,
    ReplicaSet,
    Job,
    CronJob,
    Pod,
    Event,
    PodMetric,
    NodeMetric,
    Service,
    EndpointSlice,
    Ingress,
    Gateway,
    GatewayClass,
    HttpRoute,
    NetworkPolicy,
    PersistentVolumeClaim,
    StorageClass,
    HorizontalPodAutoscaler,
    PodDisruptionBudget,
    Certificate,
    ClusterIssuer,
    CnpgCluster,
    Kafka,
    KafkaNodePool,
    KafkaTopic,
    CephCluster,
    CephFilesystem,
    CephBlockPool,
    CephObjectStore,
    VeleroBackup,
    VeleroSchedule,
    BackupStorageLocation,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NamespacedResourceKind {
    Deployment,
    StatefulSet,
    DaemonSet,
    ReplicaSet,
    Job,
    CronJob,
    Pod,
    Event,
    PodMetric,
    Service,
    EndpointSlice,
    Ingress,
    Gateway,
    HttpRoute,
    NetworkPolicy,
    PersistentVolumeClaim,
    HorizontalPodAutoscaler,
    PodDisruptionBudget,
    Certificate,
    CnpgCluster,
    Kafka,
    KafkaNodePool,
    KafkaTopic,
    CephCluster,
    CephFilesystem,
    CephBlockPool,
    CephObjectStore,
    VeleroBackup,
    VeleroSchedule,
    BackupStorageLocation,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClusterResourceKind {
    Namespace,
    Node,
    NodeMetric,
    GatewayClass,
    StorageClass,
    ClusterIssuer,
}

impl From<NamespacedResourceKind> for ResourceKind {
    fn from(kind: NamespacedResourceKind) -> Self {
        match kind {
            NamespacedResourceKind::Deployment => Self::Deployment,
            NamespacedResourceKind::StatefulSet => Self::StatefulSet,
            NamespacedResourceKind::DaemonSet => Self::DaemonSet,
            NamespacedResourceKind::ReplicaSet => Self::ReplicaSet,
            NamespacedResourceKind::Job => Self::Job,
            NamespacedResourceKind::CronJob => Self::CronJob,
            NamespacedResourceKind::Pod => Self::Pod,
            NamespacedResourceKind::Event => Self::Event,
            NamespacedResourceKind::PodMetric => Self::PodMetric,
            NamespacedResourceKind::Service => Self::Service,
            NamespacedResourceKind::EndpointSlice => Self::EndpointSlice,
            NamespacedResourceKind::Ingress => Self::Ingress,
            NamespacedResourceKind::Gateway => Self::Gateway,
            NamespacedResourceKind::HttpRoute => Self::HttpRoute,
            NamespacedResourceKind::NetworkPolicy => Self::NetworkPolicy,
            NamespacedResourceKind::PersistentVolumeClaim => Self::PersistentVolumeClaim,
            NamespacedResourceKind::HorizontalPodAutoscaler => Self::HorizontalPodAutoscaler,
            NamespacedResourceKind::PodDisruptionBudget => Self::PodDisruptionBudget,
            NamespacedResourceKind::Certificate => Self::Certificate,
            NamespacedResourceKind::CnpgCluster => Self::CnpgCluster,
            NamespacedResourceKind::Kafka => Self::Kafka,
            NamespacedResourceKind::KafkaNodePool => Self::KafkaNodePool,
            NamespacedResourceKind::KafkaTopic => Self::KafkaTopic,
            NamespacedResourceKind::CephCluster => Self::CephCluster,
            NamespacedResourceKind::CephFilesystem => Self::CephFilesystem,
            NamespacedResourceKind::CephBlockPool => Self::CephBlockPool,
            NamespacedResourceKind::CephObjectStore => Self::CephObjectStore,
            NamespacedResourceKind::VeleroBackup => Self::VeleroBackup,
            NamespacedResourceKind::VeleroSchedule => Self::VeleroSchedule,
            NamespacedResourceKind::BackupStorageLocation => Self::BackupStorageLocation,
        }
    }
}

impl From<ClusterResourceKind> for ResourceKind {
    fn from(kind: ClusterResourceKind) -> Self {
        match kind {
            ClusterResourceKind::Namespace => Self::Namespace,
            ClusterResourceKind::Node => Self::Node,
            ClusterResourceKind::NodeMetric => Self::NodeMetric,
            ClusterResourceKind::GatewayClass => Self::GatewayClass,
            ClusterResourceKind::StorageClass => Self::StorageClass,
            ClusterResourceKind::ClusterIssuer => Self::ClusterIssuer,
        }
    }
}

impl ResourceKind {
    pub fn namespaced(self) -> bool {
        !matches!(
            self,
            Self::Namespace
                | Self::Node
                | Self::NodeMetric
                | Self::GatewayClass
                | Self::StorageClass
                | Self::ClusterIssuer
        )
    }

    pub(crate) fn mapping(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Namespace => ("api", "v1", "namespaces"),
            Self::Node => ("api", "v1", "nodes"),
            Self::Deployment => ("apis", "apps/v1", "deployments"),
            Self::StatefulSet => ("apis", "apps/v1", "statefulsets"),
            Self::DaemonSet => ("apis", "apps/v1", "daemonsets"),
            Self::ReplicaSet => ("apis", "apps/v1", "replicasets"),
            Self::Job => ("apis", "batch/v1", "jobs"),
            Self::CronJob => ("apis", "batch/v1", "cronjobs"),
            Self::Pod => ("api", "v1", "pods"),
            Self::Event => ("apis", "events.k8s.io/v1", "events"),
            Self::PodMetric => ("apis", "metrics.k8s.io/v1beta1", "pods"),
            Self::NodeMetric => ("apis", "metrics.k8s.io/v1beta1", "nodes"),
            Self::Service => ("api", "v1", "services"),
            Self::EndpointSlice => ("apis", "discovery.k8s.io/v1", "endpointslices"),
            Self::Ingress => ("apis", "networking.k8s.io/v1", "ingresses"),
            Self::Gateway => ("apis", "gateway.networking.k8s.io/v1", "gateways"),
            Self::GatewayClass => ("apis", "gateway.networking.k8s.io/v1", "gatewayclasses"),
            Self::HttpRoute => ("apis", "gateway.networking.k8s.io/v1", "httproutes"),
            Self::NetworkPolicy => ("apis", "networking.k8s.io/v1", "networkpolicies"),
            Self::PersistentVolumeClaim => ("api", "v1", "persistentvolumeclaims"),
            Self::StorageClass => ("apis", "storage.k8s.io/v1", "storageclasses"),
            Self::HorizontalPodAutoscaler => ("apis", "autoscaling/v2", "horizontalpodautoscalers"),
            Self::PodDisruptionBudget => ("apis", "policy/v1", "poddisruptionbudgets"),
            Self::Certificate => ("apis", "cert-manager.io/v1", "certificates"),
            Self::ClusterIssuer => ("apis", "cert-manager.io/v1", "clusterissuers"),
            Self::CnpgCluster => ("apis", "postgresql.cnpg.io/v1", "clusters"),
            Self::Kafka => ("apis", "kafka.strimzi.io/v1beta2", "kafkas"),
            Self::KafkaNodePool => ("apis", "kafka.strimzi.io/v1beta2", "kafkanodepools"),
            Self::KafkaTopic => ("apis", "kafka.strimzi.io/v1beta2", "kafkatopics"),
            Self::CephCluster => ("apis", "ceph.rook.io/v1", "cephclusters"),
            Self::CephFilesystem => ("apis", "ceph.rook.io/v1", "cephfilesystems"),
            Self::CephBlockPool => ("apis", "ceph.rook.io/v1", "cephblockpools"),
            Self::CephObjectStore => ("apis", "ceph.rook.io/v1", "cephobjectstores"),
            Self::VeleroBackup => ("apis", "velero.io/v1", "backups"),
            Self::VeleroSchedule => ("apis", "velero.io/v1", "schedules"),
            Self::BackupStorageLocation => ("apis", "velero.io/v1", "backupstoragelocations"),
        }
    }

    pub(crate) fn api_version(self) -> String {
        self.mapping().1.to_owned()
    }
}

pub const ALL_RESOURCE_KINDS: &[ResourceKind] = &[
    ResourceKind::Namespace,
    ResourceKind::Node,
    ResourceKind::Deployment,
    ResourceKind::StatefulSet,
    ResourceKind::DaemonSet,
    ResourceKind::ReplicaSet,
    ResourceKind::Job,
    ResourceKind::CronJob,
    ResourceKind::Pod,
    ResourceKind::Event,
    ResourceKind::PodMetric,
    ResourceKind::NodeMetric,
    ResourceKind::Service,
    ResourceKind::EndpointSlice,
    ResourceKind::Ingress,
    ResourceKind::Gateway,
    ResourceKind::GatewayClass,
    ResourceKind::HttpRoute,
    ResourceKind::NetworkPolicy,
    ResourceKind::PersistentVolumeClaim,
    ResourceKind::StorageClass,
    ResourceKind::HorizontalPodAutoscaler,
    ResourceKind::PodDisruptionBudget,
    ResourceKind::Certificate,
    ResourceKind::ClusterIssuer,
    ResourceKind::CnpgCluster,
    ResourceKind::Kafka,
    ResourceKind::KafkaNodePool,
    ResourceKind::KafkaTopic,
    ResourceKind::CephCluster,
    ResourceKind::CephFilesystem,
    ResourceKind::CephBlockPool,
    ResourceKind::CephObjectStore,
    ResourceKind::VeleroBackup,
    ResourceKind::VeleroSchedule,
    ResourceKind::BackupStorageLocation,
];

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceQueryInput {
    pub kind: ResourceKind,
    pub namespace: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    pub limit: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct ResourceQuery {
    pub kind: ResourceKind,
    pub namespace: Option<String>,
    pub name: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub limit: u16,
}

impl ResourceQueryInput {
    pub fn validate(self) -> Result<ResourceQuery, ValidationError> {
        if self.namespace.as_ref().is_some_and(|v| !valid_namespace(v))
            || self.name.as_ref().is_some_and(|v| !valid_object_name(v))
            || self.kind.namespaced() != self.namespace.is_some()
            || self.name.is_some() && !self.labels.is_empty()
            || self.labels.len() > MAX_SELECTORS
            || self
                .labels
                .iter()
                .any(|(k, v)| !valid_label_key(k) || !valid_label_value(v))
        {
            return Err(ValidationError);
        }
        let limit = self.limit.unwrap_or(50);
        if !(1..=100).contains(&limit) {
            return Err(ValidationError);
        }
        Ok(ResourceQuery {
            kind: self.kind,
            namespace: self.namespace,
            name: self.name,
            labels: self.labels,
            limit,
        })
    }
}

impl ResourceQuery {
    pub(super) fn is_valid(&self) -> bool {
        self.namespace
            .as_ref()
            .is_none_or(|value| valid_namespace(value))
            && self
                .name
                .as_ref()
                .is_none_or(|value| valid_object_name(value))
            && self.kind.namespaced() == self.namespace.is_some()
            && (self.name.is_none() || self.labels.is_empty())
            && self.labels.len() <= MAX_SELECTORS
            && self
                .labels
                .iter()
                .all(|(key, value)| valid_label_key(key) && valid_label_value(value))
            && (1..=100).contains(&self.limit)
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilitiesInput {
    pub kinds: Option<Vec<ResourceKind>>,
}

#[derive(Debug, Clone)]
pub struct CapabilitiesQuery {
    pub kinds: Vec<ResourceKind>,
}

impl CapabilitiesInput {
    pub fn validate(self) -> Result<CapabilitiesQuery, ValidationError> {
        let kinds = self.kinds.unwrap_or_else(|| ALL_RESOURCE_KINDS.to_vec());
        if kinds.is_empty() || kinds.len() > ALL_RESOURCE_KINDS.len() {
            return Err(ValidationError);
        }
        let mut unique = kinds.clone();
        unique.sort_by_key(|kind| *kind as u8);
        unique.dedup();
        if unique.len() != kinds.len() {
            return Err(ValidationError);
        }
        Ok(CapabilitiesQuery { kinds })
    }
}

impl CapabilitiesQuery {
    pub(super) fn is_valid(&self) -> bool {
        let mut kinds = self.kinds.clone();
        kinds.sort_by_key(|kind| *kind as u8);
        kinds.dedup();
        !kinds.is_empty()
            && kinds.len() == self.kinds.len()
            && kinds.len() <= ALL_RESOURCE_KINDS.len()
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RestartWorkloadKind {
    Deployment,
    StatefulSet,
    DaemonSet,
}

impl RestartWorkloadKind {
    pub(crate) fn argument(self) -> &'static str {
        match self {
            Self::Deployment => "deployment",
            Self::StatefulSet => "statefulset",
            Self::DaemonSet => "daemonset",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScalableWorkloadKind {
    Deployment,
    StatefulSet,
}

impl ScalableWorkloadKind {
    pub(crate) fn argument(self) -> &'static str {
        match self {
            Self::Deployment => "deployment",
            Self::StatefulSet => "statefulset",
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecInput {
    WorkloadRestart {
        cluster: String,
        kind: RestartWorkloadKind,
        namespace: String,
        name: String,
        dry_run: Option<bool>,
    },
    WorkloadScale {
        cluster: String,
        kind: ScalableWorkloadKind,
        namespace: String,
        name: String,
        replicas: u32,
        dry_run: Option<bool>,
    },
    CronjobSuspend {
        cluster: String,
        namespace: String,
        name: String,
        suspended: bool,
        dry_run: Option<bool>,
    },
    CronjobTrigger {
        cluster: String,
        namespace: String,
        name: String,
        dry_run: Option<bool>,
    },
    PodDelete {
        cluster: String,
        namespace: String,
        name: String,
        dry_run: Option<bool>,
    },
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkloadRestartInput {
    pub cluster: String,
    pub kind: RestartWorkloadKind,
    pub namespace: String,
    pub name: String,
    pub dry_run: Option<bool>,
}

impl WorkloadRestartInput {
    pub fn validate(self) -> Result<ExecCommand, ValidationError> {
        ExecInput::WorkloadRestart {
            cluster: self.cluster,
            kind: self.kind,
            namespace: self.namespace,
            name: self.name,
            dry_run: self.dry_run,
        }
        .validate()
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkloadScaleInput {
    pub cluster: String,
    pub kind: ScalableWorkloadKind,
    pub namespace: String,
    pub name: String,
    pub replicas: u32,
    pub dry_run: Option<bool>,
}

impl WorkloadScaleInput {
    pub fn validate(self) -> Result<ExecCommand, ValidationError> {
        ExecInput::WorkloadScale {
            cluster: self.cluster,
            kind: self.kind,
            namespace: self.namespace,
            name: self.name,
            replicas: self.replicas,
            dry_run: self.dry_run,
        }
        .validate()
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CronjobSuspendInput {
    pub cluster: String,
    pub namespace: String,
    pub name: String,
    pub suspended: bool,
    pub dry_run: Option<bool>,
}

impl CronjobSuspendInput {
    pub fn validate(self) -> Result<ExecCommand, ValidationError> {
        ExecInput::CronjobSuspend {
            cluster: self.cluster,
            namespace: self.namespace,
            name: self.name,
            suspended: self.suspended,
            dry_run: self.dry_run,
        }
        .validate()
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CronjobTriggerInput {
    pub cluster: String,
    pub namespace: String,
    pub name: String,
    pub dry_run: Option<bool>,
}

impl CronjobTriggerInput {
    pub fn validate(self) -> Result<ExecCommand, ValidationError> {
        ExecInput::CronjobTrigger {
            cluster: self.cluster,
            namespace: self.namespace,
            name: self.name,
            dry_run: self.dry_run,
        }
        .validate()
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PodDeleteInput {
    pub cluster: String,
    pub namespace: String,
    pub name: String,
    pub dry_run: Option<bool>,
}

impl PodDeleteInput {
    pub fn validate(self) -> Result<ExecCommand, ValidationError> {
        ExecInput::PodDelete {
            cluster: self.cluster,
            namespace: self.namespace,
            name: self.name,
            dry_run: self.dry_run,
        }
        .validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecCommand {
    WorkloadRestart {
        cluster: String,
        kind: RestartWorkloadKind,
        namespace: String,
        name: String,
        dry_run: bool,
    },
    WorkloadScale {
        cluster: String,
        kind: ScalableWorkloadKind,
        namespace: String,
        name: String,
        replicas: u32,
        dry_run: bool,
    },
    CronjobSuspend {
        cluster: String,
        namespace: String,
        name: String,
        suspended: bool,
        dry_run: bool,
    },
    CronjobTrigger {
        cluster: String,
        namespace: String,
        name: String,
        dry_run: bool,
    },
    PodDelete {
        cluster: String,
        namespace: String,
        name: String,
        dry_run: bool,
    },
}

impl ExecInput {
    pub fn validate(self) -> Result<ExecCommand, ValidationError> {
        fn pair(namespace: &str, name: &str) -> bool {
            valid_namespace(namespace) && valid_object_name(name)
        }
        Ok(match self {
            Self::WorkloadRestart {
                cluster,
                kind,
                namespace,
                name,
                dry_run,
            } if valid_catalog_name(&cluster) && pair(&namespace, &name) => {
                ExecCommand::WorkloadRestart {
                    cluster,
                    kind,
                    namespace,
                    name,
                    dry_run: dry_run.unwrap_or(false),
                }
            }
            Self::WorkloadScale {
                cluster,
                kind,
                namespace,
                name,
                replicas,
                dry_run,
            } if valid_catalog_name(&cluster) && pair(&namespace, &name) && replicas <= 1_000 => {
                ExecCommand::WorkloadScale {
                    cluster,
                    kind,
                    namespace,
                    name,
                    replicas,
                    dry_run: dry_run.unwrap_or(false),
                }
            }
            Self::CronjobSuspend {
                cluster,
                namespace,
                name,
                suspended,
                dry_run,
            } if valid_catalog_name(&cluster)
                && pair(&namespace, &name)
                && valid_cronjob_name(&name) =>
            {
                ExecCommand::CronjobSuspend {
                    cluster,
                    namespace,
                    name,
                    suspended,
                    dry_run: dry_run.unwrap_or(false),
                }
            }
            Self::CronjobTrigger {
                cluster,
                namespace,
                name,
                dry_run,
            } if valid_catalog_name(&cluster)
                && pair(&namespace, &name)
                && valid_cronjob_name(&name) =>
            {
                ExecCommand::CronjobTrigger {
                    cluster,
                    namespace,
                    name,
                    dry_run: dry_run.unwrap_or(false),
                }
            }
            Self::PodDelete {
                cluster,
                namespace,
                name,
                dry_run,
            } if valid_catalog_name(&cluster) && pair(&namespace, &name) => {
                ExecCommand::PodDelete {
                    cluster,
                    namespace,
                    name,
                    dry_run: dry_run.unwrap_or(false),
                }
            }
            _ => return Err(ValidationError),
        })
    }
}

impl ExecCommand {
    pub fn cluster(&self) -> &str {
        match self {
            Self::WorkloadRestart { cluster, .. }
            | Self::WorkloadScale { cluster, .. }
            | Self::CronjobSuspend { cluster, .. }
            | Self::CronjobTrigger { cluster, .. }
            | Self::PodDelete { cluster, .. } => cluster,
        }
    }

    pub(super) fn is_valid(&self) -> bool {
        let pair =
            |namespace: &str, name: &str| valid_namespace(namespace) && valid_object_name(name);
        valid_catalog_name(self.cluster())
            && match self {
                Self::WorkloadRestart {
                    namespace, name, ..
                }
                | Self::PodDelete {
                    namespace, name, ..
                } => pair(namespace, name),
                Self::CronjobSuspend {
                    namespace, name, ..
                }
                | Self::CronjobTrigger {
                    namespace, name, ..
                } => pair(namespace, name) && valid_cronjob_name(name),
                Self::WorkloadScale {
                    namespace,
                    name,
                    replicas,
                    ..
                } => pair(namespace, name) && *replicas <= 1_000,
            }
    }
}

fn valid_identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && !value.starts_with('-')
        && !value.chars().any(|c| c.is_control() || c.is_whitespace())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
}

pub(super) fn valid_catalog_name(value: &str) -> bool {
    valid_identifier(value, 253)
}

pub(super) fn valid_object_name(value: &str) -> bool {
    valid_dns_subdomain(value, 253)
}

pub(super) fn valid_namespace(value: &str) -> bool {
    valid_dns_label(value)
}

fn valid_cronjob_name(value: &str) -> bool {
    value.len() <= 52 && valid_object_name(value)
}

fn valid_label_key(value: &str) -> bool {
    let (prefix, name) = value
        .split_once('/')
        .map_or((None, value), |(prefix, name)| (Some(prefix), name));
    prefix.is_none_or(|prefix| valid_dns_subdomain(prefix, 253)) && valid_label_segment(name, false)
}

fn valid_label_value(value: &str) -> bool {
    valid_label_segment(value, true)
}

fn valid_dns_subdomain(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && value.split('.').all(valid_dns_label)
}

fn valid_dns_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

fn valid_label_segment(value: &str, empty_allowed: bool) -> bool {
    if value.is_empty() {
        return empty_allowed;
    }
    value.len() <= 63
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'))
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_injection_is_rejected() {
        for value in [
            "--kubeconfig=/tmp/x",
            "-n",
            "a b",
            "a\nb",
            "a;id",
            "UPPER",
            "bad_name",
            "bad.",
            "",
        ] {
            assert!(
                ResourceQueryInput {
                    kind: ResourceKind::Pod,
                    namespace: Some(value.into()),
                    name: None,
                    labels: BTreeMap::new(),
                    limit: None
                }
                .validate()
                .is_err(),
                "accepted {value:?}"
            );
        }
    }

    #[test]
    fn exact_action_validation() {
        assert!(
            ExecInput::WorkloadScale {
                cluster: "cluster".into(),
                kind: ScalableWorkloadKind::Deployment,
                namespace: "ns".into(),
                name: "app".into(),
                replicas: 1_001,
                dry_run: None
            }
            .validate()
            .is_err()
        );
        assert!(
            ExecInput::PodDelete {
                cluster: "cluster".into(),
                namespace: "ns".into(),
                name: "--all".into(),
                dry_run: None
            }
            .validate()
            .is_err()
        );
        assert!(
            serde_json::from_str::<ExecInput>(
                r#"{"action":"pod_delete","namespace":"ns","name":"pod","force":true}"#
            )
            .is_err()
        );
        assert!(
            ExecInput::CronjobTrigger {
                cluster: "cluster".into(),
                namespace: "ns".into(),
                name: "a".repeat(53),
                dry_run: Some(false),
            }
            .validate()
            .is_err()
        );
        assert!(
            serde_json::from_str::<QueryInput>(
                r#"{"action":"resource_get","cluster":"-other","kind":"pod","namespace":"ns","name":"pod"}"#
            )
            .unwrap()
            .validate()
            .is_err()
        );
        assert!(
            serde_json::from_str::<QueryInput>(
                r#"{"action":"resource_list","cluster":"cluster","kind":"pod","name":"pod"}"#
            )
            .is_err()
        );
        for payload in [
            r#"{"cluster":"cluster","kind":"pod"}"#,
            r#"{"cluster":"cluster","kind":"node","namespace":"ns"}"#,
            r#"{"cluster":"cluster","kind":"pod","namespace":"ns","credential":"secret"}"#,
        ] {
            assert!(serde_json::from_str::<ResourceListInput>(payload).is_err());
            assert!(serde_json::from_str::<ResourceGetInput>(payload).is_err());
        }
        assert!(
            serde_json::from_str::<ResourceListInput>(
                r#"{"cluster":"cluster","kind":"pod","namespace":"ns"}"#
            )
            .is_ok()
        );
        assert!(
            serde_json::from_str::<ResourceListInput>(r#"{"cluster":"cluster","kind":"node"}"#)
                .is_ok()
        );
        assert!(
            serde_json::from_str::<ResourceGetInput>(
                r#"{"cluster":"cluster","kind":"pod","namespace":"ns","name":"pod"}"#
            )
            .is_ok()
        );
        assert!(
            serde_json::from_str::<ResourceGetInput>(
                r#"{"cluster":"cluster","kind":"node","name":"node"}"#
            )
            .is_ok()
        );
        assert!(
            serde_json::from_str::<ResourceGetInput>(
                r#"{"cluster":"cluster","kind":"pod","name":"pod"}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<QueryInput>(
                r#"{"action":"cluster_list","cluster":"unexpected"}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<ExecInput>(
                r#"{"action":"workload_scale","cluster":"cluster","kind":"daemon_set","namespace":"ns","name":"agent","replicas":2}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<ExecInput>(
                r#"{"action":"workload_scale","cluster":"cluster","kind":"replica_set","namespace":"ns","name":"agent","replicas":2}"#
            )
            .is_err()
        );
        for replicas in [0, 1_000] {
            assert!(
                ExecInput::WorkloadScale {
                    cluster: "cluster".into(),
                    kind: ScalableWorkloadKind::StatefulSet,
                    namespace: "ns".into(),
                    name: "app".into(),
                    replicas,
                    dry_run: None,
                }
                .validate()
                .is_ok()
            );
        }
        assert!(
            serde_json::from_str::<ExecInput>(
                r#"{"action":"workload_restart","cluster":"cluster","kind":"replica_set","namespace":"ns","name":"app"}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<ExecInput>(
                r#"{"action":"cronjob_trigger","cluster":"cluster","namespace":"ns","name":"job","suspended":true}"#
            )
            .is_err()
        );
    }

    #[test]
    fn list_and_selector_limits() {
        assert!(
            ResourceQueryInput {
                kind: ResourceKind::Pod,
                namespace: None,
                name: None,
                labels: BTreeMap::new(),
                limit: Some(101)
            }
            .validate()
            .is_err()
        );
        let labels = (0..9).map(|n| (format!("key{n}"), "v".into())).collect();
        assert!(
            ResourceQueryInput {
                kind: ResourceKind::Pod,
                namespace: Some("ns".into()),
                name: None,
                labels,
                limit: None
            }
            .validate()
            .is_err()
        );
        let labels = (0..8).map(|n| (format!("key{n}"), "v".into())).collect();
        assert!(
            ResourceQueryInput {
                kind: ResourceKind::Pod,
                namespace: Some("ns".into()),
                name: None,
                labels,
                limit: None
            }
            .validate()
            .is_ok()
        );
        for key in ["/name", "prefix/", "a/b/c", "bad_prefix/name", "-bad"] {
            assert!(
                ResourceQueryInput {
                    kind: ResourceKind::Pod,
                    namespace: None,
                    name: None,
                    labels: [(key.into(), "value".into())].into(),
                    limit: None,
                }
                .validate()
                .is_err(),
                "accepted label key {key:?}"
            );
        }
        assert!(
            ResourceQueryInput {
                kind: ResourceKind::Pod,
                namespace: Some("ns".into()),
                name: None,
                labels: [("example.com/app_name".into(), String::new())].into(),
                limit: None,
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn resource_input_schemas_partition_every_kind_by_scope() {
        fn enum_values(value: &serde_json::Value, output: &mut Vec<String>) {
            match value {
                serde_json::Value::Object(object) => {
                    if let Some(values) = object.get("enum").and_then(serde_json::Value::as_array) {
                        output.extend(
                            values
                                .iter()
                                .filter_map(serde_json::Value::as_str)
                                .map(str::to_owned),
                        );
                    }
                    for value in object.values() {
                        enum_values(value, output);
                    }
                }
                serde_json::Value::Array(values) => {
                    for value in values {
                        enum_values(value, output);
                    }
                }
                _ => {}
            }
        }

        let expected_namespaced = ALL_RESOURCE_KINDS
            .iter()
            .copied()
            .filter(|kind| kind.namespaced())
            .map(|kind| {
                serde_json::to_value(kind)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect::<Vec<_>>();
        let expected_cluster = ALL_RESOURCE_KINDS
            .iter()
            .copied()
            .filter(|kind| !kind.namespaced())
            .map(|kind| {
                serde_json::to_value(kind)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect::<Vec<_>>();
        for (schema, mut expected) in [
            (
                serde_json::to_value(schemars::schema_for!(NamespacedResourceKind)).unwrap(),
                expected_namespaced.clone(),
            ),
            (
                serde_json::to_value(schemars::schema_for!(ClusterResourceKind)).unwrap(),
                expected_cluster.clone(),
            ),
        ] {
            let mut advertised = Vec::new();
            enum_values(&schema, &mut advertised);
            advertised.sort();
            expected.sort();
            assert_eq!(advertised, expected);
        }
        for schema in [
            serde_json::to_value(schemars::schema_for!(ResourceListInput)).unwrap(),
            serde_json::to_value(schemars::schema_for!(ResourceGetInput)).unwrap(),
        ] {
            let mut advertised = Vec::new();
            enum_values(&schema, &mut advertised);
            advertised.sort();
            advertised.dedup();
            let mut expected = expected_namespaced
                .iter()
                .chain(&expected_cluster)
                .cloned()
                .collect::<Vec<_>>();
            expected.sort();
            assert_eq!(advertised, expected);
            assert_eq!(advertised.len(), ALL_RESOURCE_KINDS.len());
            let branches = schema["anyOf"].as_array().unwrap();
            assert_eq!(branches.len(), 2);
            assert_eq!(branches[0]["additionalProperties"], false);
            assert_eq!(branches[1]["additionalProperties"], false);
            assert!(
                branches[0]["required"]
                    .as_array()
                    .unwrap()
                    .contains(&serde_json::json!("namespace"))
            );
            assert!(branches[1]["properties"].get("namespace").is_none());
        }
    }
}
