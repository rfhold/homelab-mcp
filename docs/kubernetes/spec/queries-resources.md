# Kubernetes Query and Resource Specifications

## Query Actions

`kubernetes_query` exposes four actions:

| Action | Contract |
| --- | --- |
| `cluster_list` | Return the configured catalog in stable cluster-name order. It performs no cluster request. |
| `capability_list` | Report support for all approved kinds, or a unique nonempty requested subset, on one exact cluster. |
| `resource_list` | List one approved kind on one exact cluster, with required namespace for namespaced kinds, exact labels, and a 1-through-100 result limit. |
| `resource_get` | Read one exact approved object by cluster, kind, namespace when required, and name. |

`resource_list` will default its result limit to 50. It will accept at most eight exact key-value label matches. It will not accept set expressions, inequality, caller-built selector text, field selectors, or name patterns.

Cluster-scoped resources reject a namespace. Both `resource_list` and `resource_get` require an exact namespace for each namespaced kind. Their generated schemas expose this requirement through disjoint namespaced and cluster-scoped kind subsets while preserving the existing JSON object fields.

The typed inputs in `src/integrations/kubernetes/actions.rs` will define the action and resource enums. Callers cannot substitute plural names, API groups, versions, or paths.

## Approved Resource Kinds

The fixed read catalog will contain:

| Domain | Kinds |
| --- | --- |
| Cluster | Namespace, Node |
| Workloads | Deployment, StatefulSet, DaemonSet, ReplicaSet, Job, CronJob |
| Pod health and events | Pod, Event |
| Metrics | PodMetric, NodeMetric |
| Service discovery | Service, EndpointSlice |
| Traffic | Ingress, Gateway, GatewayClass, HTTPRoute, NetworkPolicy |
| Storage and availability | PersistentVolumeClaim, StorageClass, HorizontalPodAutoscaler, PodDisruptionBudget |
| cert-manager | Certificate, ClusterIssuer |
| CloudNativePG | Cluster |
| Strimzi | Kafka, KafkaNodePool, KafkaTopic |
| Rook | CephCluster, CephFilesystem, CephBlockPool, CephObjectStore |
| Velero | Backup, Schedule, BackupStorageLocation |

The public typed names will disambiguate collisions. CloudNativePG Cluster will use `cnpg_cluster`. Velero resources will use `velero_backup`, `velero_schedule`, and `backup_storage_location`.

The approved API versions and resource paths will remain fixed in `ResourceKind::mapping` under `src/integrations/kubernetes/actions.rs`. Capability discovery will report unsupported APIs without expanding the catalog.

## Rook and Native Ceph State

Reads of Rook `CephCluster`, `CephFilesystem`, `CephBlockPool`, and `CephObjectStore` resources remain coarse Kubernetes controller-state views. They report only approved custom-resource status and conditions. They do not report authoritative native OSD state, devices, safe-to-destroy decisions, cluster flags, current Dashboard metrics, or Dashboard tasks.

The [Ceph Dashboard tools](../../ceph/README.md) own native Ceph operational state and every approved native mutation. Kubernetes resource reads do not expand Ceph authority and cannot be used as a substitute for the Ceph destroy or purge safety checks.

## Normalized Results

Every resource result will contain an approved subset of identity, API version, namespace, name, creation time, status, details, and conditions. `src/integrations/kubernetes/normalize.rs` with `Resource` and `ResourceList` will own the output model.

The normalized details can include:

- replica, readiness, availability, job, CronJob, pod phase, node, IP, and restart summaries;
- Event reason, bounded message, related kind, related name, and count;
- pod and node CPU or memory quantities;
- Service type and cluster IP, plus EndpointSlice endpoint readiness counts;
- Ingress and Gateway class, GatewayClass controller, and HTTPRoute rule count;
- NetworkPolicy rule counts;
- PVC phase, storage class, and capacity;
- StorageClass provisioner and reclaim policy;
- HPA replica state and PDB disruption allowance; and
- fixed status and condition summaries for approved platform resources.

Conditions will contain at most 20 entries. Each condition will use allowlisted type, status, reason, and transition time fields.

Each Event message will contain at most 1,024 UTF-8 bytes. One result will contain at most 32 KiB of Event message text. Truncation will preserve valid UTF-8 and will remain explicit.

Results will not include raw specifications, arbitrary status extensions, arbitrary labels, arbitrary annotations, managed fields, container environments, volumes, or Secret references.

## Pagination and Truncation

Lists will follow Kubernetes continuation tokens for at most five pages and 500 inspected objects. They will return at most the validated limit, which cannot exceed 100.

The service will reject malformed, repeated, oversized, or unsafe continuation tokens from upstream. It will not expose continuation tokens to callers.

The result will distinguish source truncation from result truncation. `truncated` will equal true when either condition applies.

## Capability Semantics

`capability_list` will inspect discovery documents only for approved API groups and versions. It will report each requested typed kind, fixed API version, and support status.

Capability discovery will not grant authority to query unapproved resources. API discovery success also will not prove that runtime RBAC permits an object read.
