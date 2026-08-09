# Deployment

## Status

The repository implements container, Pulumi, and preview-pipeline foundations. The `preview` stack is deployed by the main pipeline and serves its health endpoints through the default gateway. The `prod` stack is initialized with zero resources and has not been previewed or applied.

Pulumi has applied the declared resources to preview. Production remains declaration-only.

The deployed preview process remains health-only. It does not serve `/mcp`, OAuth, browser callbacks, PostgreSQL-backed runtime state, or Grafana queries.

The repository implements the OAuth and LogQL contracts at a reviewed immutable Kuri Git revision.

The container and pipeline declarations can build the runtime, but the new runtime has not been deployed.

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

The deployed health-only host makes health and readiness unconditional. The working-tree `/health` remains unconditional.

The working-tree `/ready` performs bounded live PostgreSQL and signing-key-readiness checks. It does not probe Authentik or Grafana.

## Container Image

`Dockerfile` follows the Kuri Rust 1.96 and Debian bookworm pattern. It builds the release binary and copies it into the minimal runtime image; Rust tests run directly through Cargo outside the image build.

The generic `mcp` dependency embeds its PostgreSQL migrations. No application migration directory enters the image. The runtime installs only CA certificates and runs as UID/GID 65532.

The image includes OCI source and revision labels. It has no Docker `HEALTHCHECK`; Kubernetes owns health checks.

Cargo uses CLI Git for the private Kuri dependency. The release build stages retain BuildKit secret mounts and architecture-specific caches.

## Credential Boundaries

Pulumi creates the Grafana Viewer service account and token and places the token in the application Secret. These resources exist in preview only.

Pulumi places runtime credentials in the application Secret. It keeps the wrapping-key file in a separate Secret and read-only mount.

The working-tree service issues local ES256 access tokens and uses Authentik only for browser identity. Its `/mcp` boundary accepts only locally issued tokens.

PostgreSQL holds generic OAuth/OIDC state and encrypted signing material in the `mcp` schema. Migrations V1-V3 own hosted OAuth, signing, and registration state; V4 adds one-shot OIDC attempts.

The Grafana client sends its token only in the `Authorization` header. It uses the fixed datasource UID `loki`, Grafana's datasource proxy, and disabled redirects.

Runtime logs, redirects, MCP results, image layers, rendered outputs, and health responses must not expose secret values.

The non-expiring Grafana token requires explicit rotation and revocation procedures before production readiness.

## Preview Pipeline

`.tekton/homelab-mcp-preview.yaml` targets `main` push and incoming events. It defines one preview path and no release path.

The pipeline clones the requested revision and scans Cargo, container, Tekton, and Pulumi inputs for private key patterns. The amd64 and arm64 image builds then run in parallel.

The final `general-ci:latest` step maps Grafana provider credentials, runs `pulumi preview --stack preview`, then runs `pulumi up --stack preview`.

The main pipeline has completed successfully and applied the preview stack.

That prior run proves the deployment foundation and health path. It predates the working-tree runtime and does not prove OAuth, PostgreSQL runtime, MCP, or LogQL behavior.

No release pipeline exists.

## Working-Tree Runtime and Preview Gate

The working-tree runtime targets stateless MCP Streamable HTTP revision `2026-07-28` at exact resource `/mcp`.

It requires locally issued `mcp:use` tokens and configures DCR, CIMD, and native loopback clients through PostgreSQL-backed OAuth state.

Generic Kuri owns strict OIDC login, callback, one-shot transaction state, ID-token verification, the mapper seam, and hosted continuation. Homelab supplies Authentik configuration and stable issuer-plus-subject mapping.

The runtime exposes one progressive `grafana_exec` tool. Its only domain action is `logql`, with the limits and stable results from the [LogQL specification](../grafana-exec/spec/logql.md).

No commit, push, pipeline execution, or preview deployment is authorized by these declarations. Preview acceptance requires the layered evidence from the [testing document](../quality/testing.md) and explicit approval for each external action. Existing public health checks do not satisfy that acceptance boundary.

## Delivery Inputs

Tekton maps a Grafana bootstrap credential that can create the declared service account and token. The pipeline must not persist it in build output.

BuildKit receives private Git credentials through secure secret mounts. The build must not copy those credentials into image layers or logs.

The Cargo project resolves the private Kuri dependency from its exact reviewed Git revision recorded in the lockfile.

## Availability and Data

The declared topology uses one replica. It does not provide service-level high availability.

The preview config declares 14-day backup retention and 10 GiB database storage. Production declares 30-day retention and 20 GiB storage.

Both configs declare a daily backup schedule and an S3-compatible backup endpoint. Restore procedures and recovery targets remain unresolved.

## Approval Boundaries

Stack initialization, Pulumi preview, deployment, credential creation, cluster mutation, and Tekton runs each require explicit target-specific authority.

Each external action requires explicit target-specific approval. Repository pipeline declarations do not grant that approval.

Production preview, apply, credential creation, pipeline execution, and release remain excluded. Committing, pushing, and updating preview also remain explicit gates.
