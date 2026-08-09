# Deployment

## Status

The repository implements container, Pulumi, and preview-pipeline foundations. The `preview` stack is deployed by the main pipeline and serves its health endpoints through the default gateway. The `prod` stack is initialized with zero resources and has not been previewed or applied.

Pulumi has applied the declared resources to preview. Production remains declaration-only.

## Stack Targets

| Stack | Namespace | Hostname |
| --- | --- | --- |
| Preview | `homelab-mcp-preview` | `preview-homelab-mcp.holdenitdown.net` |
| Production | `homelab-mcp` | `homelab-mcp.holdenitdown.net` |

`infra/pulumi/` defines both stacks with Pulumi TypeScript and Bun. Stack configuration files contain no image or secret values.

## Declared Resources

The preview stack creates, and an approved production `pulumi up` would create:

- the target Namespace;
- an ObjectBucketClaim for database backups;
- a one-instance CloudNativePG Cluster, Database, and ScheduledBackup;
- an Authentik confidential browser application and RSA signing certificate;
- a Grafana Viewer service account and non-expiring token;
- a versioned 32-byte OAuth wrapping-key Secret and an application Secret;
- a hardened one-replica `Recreate` Deployment;
- an egress NetworkPolicy and ClusterIP Service; and
- an HTTPRoute to `ingress/default-gateway` with request timeout `0s`.

The Deployment runs as UID and GID 65532. It disables service-account token mounts, privilege escalation, root filesystems, and Linux capabilities.

The Deployment declares startup, readiness, and liveness probes, explicit resource limits, a bounded temporary volume, and Stakater Reloader annotations.

The current host makes health and readiness unconditional. The probes do not prove database, OAuth, or Grafana readiness.

## Container Image

`Dockerfile` follows the Kuri Rust 1.96 and Debian bookworm pattern. The runtime installs only CA certificates and runs as UID/GID 65532.

The image includes OCI source and revision labels. It has no Docker `HEALTHCHECK`; Kubernetes owns health checks.

BuildKit accepts optional `gitconfig` and `git-credentials` secret mounts for future private Rust dependencies. The build does not copy those files into image layers.

## Credential Boundaries

Pulumi creates the Grafana Viewer service account and token and places the token in the application Secret. These resources exist in preview only.

Pulumi places runtime credentials in the application Secret. It keeps the wrapping-key file in a separate Secret and read-only mount.

The non-expiring Grafana token requires explicit rotation and revocation procedures before production readiness.

## Preview Pipeline

`.tekton/homelab-mcp-preview.yaml` targets `main` push and incoming events. It defines one preview path and no release path.

The pipeline clones the requested revision, scans tracked deployment inputs for private key patterns, and starts parallel amd64 and arm64 builds.

Shared BuildKit workers receive private Git credentials through secret mounts. The pipeline creates a multi-architecture manifest and verifies its digest, runtime user, and revision label.

The final `general-ci:latest` step maps Grafana provider credentials, runs `pulumi preview --stack preview`, then runs `pulumi up --stack preview`.

The main pipeline has completed successfully and applied the preview stack.

No release pipeline exists.

## Delivery Inputs

Tekton maps a Grafana bootstrap credential that can create the declared service account and token. The pipeline must not persist it in build output.

BuildKit will receive private Git credentials through a secure secret mount. The build must not copy those credentials into image layers, build context output, or logs.

The current Cargo project has no private `mcp` dependency. Future private Git access will pin that crate to commit `302fd702ffdcf89ab4829f3a299486fc297406f9`.

## Availability and Data

The declared topology uses one replica. It does not provide service-level high availability.

The preview config declares 14-day backup retention and 10 GiB database storage. Production declares 30-day retention and 20 GiB storage.

Both configs declare a daily backup schedule and an S3-compatible backup endpoint. Restore procedures and recovery targets remain unresolved.

## Approval Boundaries

Stack initialization, Pulumi preview, deployment, credential creation, cluster mutation, and Tekton runs each require explicit target-specific authority.

Each external action requires explicit target-specific approval. Repository pipeline declarations do not grant that approval.
