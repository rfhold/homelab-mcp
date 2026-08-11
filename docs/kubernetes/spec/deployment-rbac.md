# Kubernetes Deployment and RBAC Contract

## Status

This document defines the locally declared deployment boundary. Pulumi declarations and mock tests exist; deployment, live-cluster, and effective-RBAC evidence remain pending. Production remains unapplied.

## Cluster Catalog Configuration

Runtime configuration will define from one through 32 unique cluster entries. Each entry will bind a public catalog name to a fixed absolute kubeconfig path and fixed context.

The initial catalog will represent Pantheon and Romulus. Configuration will define their exact stable selector names and fixed contexts.

Configuration validation will reject an empty catalog, duplicate names, unsafe names, relative paths, invalid contexts, and more than 32 entries.

The runtime image will contain a reviewed `kubectl` executable. The process environment, kubeconfig path, context, request timeout, operation, resource kind, and output format will come from server code or validated typed fields.

## Credential Separation

The kubeconfig that Tekton uses to deploy this service provides bootstrap authority only. The runtime will not mount, copy, or reuse that provider credential.

Each target cluster will issue dedicated reduced runtime credentials. The deployment will mount only those runtime kubeconfigs and will keep them separate from application secrets and OAuth key material.

Each target cluster will use one combined runtime ServiceAccount for both query and exec actions. That ServiceAccount will have one exact RBAC grant set for the approved catalog.

Credential creation, retrieval, rotation, revocation, and mounting require target-specific authorization. No repository document or pipeline grants that authority.

## Fixed Read RBAC

The runtime ServiceAccount receives cluster-wide `get` and `list` access only for the approved [resource catalog](queries-resources.md#approved-resource-kinds). Its application grant permits `get` only on exact non-resource discovery paths: `/api`, `/apis`, `/version`, `/api/v1`, and each fixed `/apis/<group>/<version>` represented by `ResourceKind::mapping`. It grants no wildcard non-resource URL.

Standard Kubernetes installations may independently bind authenticated principals to broader discovery access through `system:discovery`. This application grant does not remove or narrow inherited cluster defaults; effective-RBAC verification remains required.

The read grant includes the fixed core, apps, batch, events, metrics, discovery, networking, Gateway API, storage, autoscaling, policy, cert-manager, CloudNativePG, Strimzi, Rook, and Velero resources.

The ServiceAccount will receive no read access to Secrets, ConfigMaps, arbitrary custom resources, raw API paths, pod logs, or pod subresources.

## Fixed Write RBAC

The same ServiceAccount will receive cluster-wide write access only for these operations:

| Resource | Verbs | Purpose |
| --- | --- | --- |
| Deployments, StatefulSets, DaemonSets | `get`, `patch` | Exact-object rollout restart. |
| Deployments/scale, StatefulSets/scale | `get`, `patch` | Exact-object scale. |
| CronJobs | `get`, `patch` | Exact-object suspend or resume. |
| Jobs | `create` | Create one Job from an exact CronJob. |
| Pods | `get`, `delete` | Delete one exact Pod. |

Server dry-run uses the same write verbs. RBAC does not create a separate dry-run permission.

The ServiceAccount will receive no wildcard API groups, resources, or verbs. It will receive no impersonation, escalation, binding, Secret, ConfigMap, exec, attach, log, proxy, port-forward, eviction, node-write, general apply, or unrelated delete authority.

RBAC cannot constrain a granted verb to the MCP input grammar or one object name. The typed server boundary must enforce exact cluster, kind, namespace, name, action, and bounds before process launch.

## Network and Process Boundary

NetworkPolicy allows each configured Kubernetes API destination CIDR on that server's validated effective HTTPS port: the explicit URL port, or 443 when omitted. Ports must be from 1 through 65535. Pantheon and Romulus remain configured on 6443. Cluster-specific endpoint behavior, including post-DNAT addresses, must inform each declared rule.

`src/integrations/kubernetes/runner.rs` with `Runner` will own environment clearing, fixed flags, process concurrency, deadlines, bounded output drains, cancellation, and child termination.

The runtime will not use a shell. Caller values will occupy only validated argument positions for the selected fixed action.

## Operational Gates

Before preview deployment, an authorized operator must review the exact catalog, mounted credential identities, ClusterRoles, bindings, and NetworkPolicy destinations.

Preview validation must use impersonation or equivalent authorization checks before any real mutation. It must prove allowed operations and representative denied exclusions.

Any ServiceAccount creation, token or kubeconfig issuance, RBAC mutation, preview read, dry-run, real mutation, credential rotation, or revocation requires exact target-specific authority.

Production preview, apply, credential creation, and validation remain excluded.
