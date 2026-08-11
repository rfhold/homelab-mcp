# Deployment

## Status

The repository implements container, Pulumi, and preview-pipeline foundations. The `preview` stack is deployed by the main pipeline and serves its health endpoints through the default gateway. The `prod` stack is initialized with zero resources and has not been previewed or applied.

Pulumi has applied the previously deployed revision's resources to preview. The alerting and Editor changes remain worktree-only. Production remains declaration-only.

The deployed preview serves health, readiness, hosted OAuth/OIDC routes, authenticated `/mcp`, PostgreSQL-backed runtime state, and the Grafana adapter.

The repository implements the OAuth and LogQL contracts at a reviewed immutable Kuri Git revision.

PipelineRun `homelab-mcp-preview-tfd8k` deployed commit `4f2e192` successfully.

## Stack Targets

| Stack | Namespace | Hostname |
| --- | --- | --- |
| Preview | `homelab-mcp-preview` | `preview-homelab-mcp.holdenitdown.net` |
| Production | `homelab-mcp` | `homelab-mcp.holdenitdown.net` |

`infra/pulumi/` defines both stacks with Pulumi TypeScript and Bun. Stack configuration files contain no image or secret values.

## Declared Resources

The current Pulumi declarations would create:

- the target Namespace;
- an ObjectBucketClaim for database backups;
- a one-instance CloudNativePG Cluster, Database, and ScheduledBackup;
- an Authentik confidential browser application and RSA signing certificate;
- a Grafana Editor service account and non-expiring token;
- a versioned 32-byte OAuth wrapping-key Secret and an application Secret;
- a hardened one-replica `Recreate` Deployment;
- an egress NetworkPolicy and ClusterIP Service; and
- an HTTPRoute to `ingress/default-gateway` with request timeout `0s`.

The Deployment runs as UID and GID 65532. It disables service-account token mounts, privilege escalation, root filesystems, and Linux capabilities.

The Deployment declares startup, readiness, and liveness probes, explicit resource limits, a bounded temporary volume, and Stakater Reloader annotations.

The deployed `/health` remains unconditional. `/ready` performs bounded live PostgreSQL and signing-key-readiness checks; it does not probe Authentik or Grafana.

## Container Image

`Dockerfile` follows the Kuri Rust 1.96 and Debian bookworm pattern. It builds the release binary and copies it into the minimal runtime image; Rust tests run directly through Cargo outside the image build.

The generic `mcp` dependency embeds its PostgreSQL migrations. No application migration directory enters the image. The runtime installs only CA certificates and runs as UID/GID 65532.

The image includes OCI source and revision labels. It has no Docker `HEALTHCHECK`; Kubernetes owns health checks.

Cargo uses CLI Git for the private Kuri dependency. The release build stages retain BuildKit secret mounts and architecture-specific caches.

## Credential Boundaries

The deployed preview revision created the Grafana service account with Viewer access. The worktree promotes it to Editor and places the same account's token in the application Secret so one server-held credential can read alerting state and create silences. This security expansion has not been applied or verified live.

Pulumi places runtime credentials in the application Secret. It keeps the wrapping-key file in a separate Secret and read-only mount.

The deployed service issues local ES256 access tokens and uses Authentik only for browser identity. Its `/mcp` boundary accepts only locally issued tokens.

PostgreSQL holds generic OAuth/OIDC state and encrypted signing material in the `mcp` schema. Migrations V1-V3 own hosted OAuth, signing, and registration state; V4 adds one-shot OIDC attempts.

The Grafana client sends its token only in the `Authorization` header. It uses fixed datasource, dashboard, render, and alerting API routes with disabled redirects. Callers cannot select the origin, token, slug, organization, path, datasource, headers, or method.

Grafana rendering requires Grafana 13.1.1 and image renderer 5.7.1 deployed separately. The Kuri consumer must include model-visible image support from revision `6eebdb0` or newer, and the selected model must support images. These renderer prerequisites and live rendering have not been validated by this repository change.

Runtime logs, redirects, MCP results, image layers, rendered outputs, and health responses must not expose secret values.

The non-expiring Grafana token requires explicit rotation and revocation procedures before production readiness.

## Tekton Access

The worktree implements the Tekton runtime and Pulumi declarations in this section. They remain unapplied; production remains unapplied.

The [Tekton tool specifications](../tekton/README.md) own action behavior, authority checks, and mutation outcomes. This section owns the deployment and credential boundaries.

Pulumi declares separate env-backed Stashes for Tekton credentials. `FORGEJO_HOLDENITDOWN_TOKEN` seeds the Forgejo Stash. `PAC_INCOMING_SECRET` seeds the PAC Stash. Pulumi projects both outputs into `homelab-mcp-app` without placing values in stack configuration or rendered documentation.

The runtime uses fixed Forgejo origin `https://git.holdenitdown.net`. It sends PAC dispatches only to `http://pipelines-as-code-controller.pipelines-as-code.svc.cluster.local:8080/incoming`.

The Deployment declares a dedicated ServiceAccount and an explicit one-hour projected Kubernetes token that the runtime reloads from disk for each Kubernetes request. It does not use the default automatic token mount.

A namespace Role in `pipelines-as-code` grants only the resource reads required for repositories, runs, tasks, pod ownership, and logs. It grants `PipelineRun` patch only for cancellation. The runtime receives no Secret read, Secret create, Secret delete, cluster role, or unrelated write permission.

The CI deployment runs `pulumi up --skip-preview`. A separate authorized local preview must be reviewed before pushing an infrastructure revision because Kubernetes cannot reliably admit a new RoleBinding against a Role that exists only in server-side dry-run. Pulumi mock tests remain the executable declaration evidence for the binding's exact role and subject.

Run, task, pod, and PAC Repository access is fixed to `pipelines-as-code`. The Role remains namespace-scoped, and every tool action enforces the canonical ownership checks.

Pantheon evaluates Kubernetes Service egress after DNAT to control-plane endpoints. The NetworkPolicy therefore allows TCP 6443 only to the stack-configured `kubernetesApiEndpointCidr`; allowing only the Service port 443 does not make the in-cluster API reachable.

Local Pulumi mocks can verify declarations but cannot verify effective cluster authorization or controller behavior. Any Stash seed, preview, apply, credential creation, cluster read, dispatch, rerun, cancellation, or live verification requires exact target-specific authority.

## Preview Pipeline

`.tekton/homelab-mcp-preview.yaml` targets `main` push and incoming events. It defines one preview path and no release path.

The pipeline clones the requested revision and scans Cargo, container, Tekton, and Pulumi inputs for private key patterns. The amd64 and arm64 image builds then run in parallel.

The final `general-ci:latest` step maps Grafana provider credentials and runs `pulumi up --stack preview --skip-preview`. It relies on the separately reviewed local preview and repository checks for pre-apply evidence.

The main pipeline completed successfully for commit `4f2e192` and applied the preview stack. The current image digest is `sha256:9a9a5a9aacf508494f904a208c6c91d972ea8e568cd61f98eaa077a761c3b7fe`.

That run proves image delivery, runtime startup, PostgreSQL-backed readiness, public health/readiness, OAuth metadata, and the unauthenticated MCP Bearer challenge. It does not prove browser login, token issuance or refresh, authenticated MCP calls, or live LogQL behavior.

No release pipeline exists.

## Preview Runtime and Remaining Gate

The preview runtime targets stateless MCP Streamable HTTP revision `2026-07-28` at exact resource `/mcp`.

It requires locally issued `mcp:use` tokens and configures DCR, CIMD, and native loopback clients through PostgreSQL-backed OAuth state.

Generic Kuri owns strict OIDC login, callback, one-shot transaction state, ID-token verification, the mapper seam, and hosted continuation. Homelab supplies Authentik configuration and stable issuer-plus-subject mapping.

The current worktree exposes nine read-only actions through `grafana_query`, two image actions through `grafana_render`, and only `silence.create` through separately advertised, operationally consequential `grafana_exec`. The existing `mcp:use` scope authorizes all three Grafana tools. Their canonical limits, results, and errors are defined by the [Grafana query](../grafana-query/README.md) and [render](../grafana-render/README.md) specifications. The deployed preview revision predates this expansion.

Silence creation performs no automatic retry. If it returns `mutation_outcome_unknown`, use `silence.list` to inspect current silences before deciding whether to retry because Grafana may already have applied the request. A silence suppresses matching notifications; it does not stop rule evaluation or delete alert data.

Commit `4f2e192` is deployed to preview. No deployment or live operation occurred for the dashboard, rendering, and alerting expansion. Full browser OAuth, authenticated preview MCP calls, live Grafana behavior, renderer operation, and Editor permission operation still require the layered evidence from the [testing document](../quality/testing.md) and explicit approval for each external action; basic public endpoint checks do not satisfy that boundary.

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
