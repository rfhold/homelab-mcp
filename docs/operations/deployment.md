# Deployment

## Status

The repository implements container, Pulumi, and preview-pipeline foundations. The `preview` stack is deployed by the main pipeline and serves its health endpoints through the default gateway. The `prod` stack is initialized with zero resources and has not been previewed or applied.

Pulumi has applied commit `798dd92` to preview. The approved recording-label and synthetic-group normalization fixes remain undeployed. Production remains declaration-only.

The deployed preview serves health, readiness, hosted OAuth/OIDC routes, authenticated `/mcp`, PostgreSQL-backed runtime state, and the Grafana adapter.

The repository implements the OAuth and LogQL contracts at a reviewed immutable Kuri Git revision.

The preview workflow deployed commit `798dd92` successfully.

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

`Dockerfile` uses Rust 1.96 build stages and a Python 3.13 Debian bookworm runtime. It builds the release binary; Rust tests run directly through Cargo outside the image build.

The generic `mcp` dependency embeds its PostgreSQL migrations. The application binary also embeds migrations from `migrations/`, including `homelab.machines`, through `sqlx::migrate!` in `src/database.rs`.

The runtime contains Python 3.13, uv, the locked pyinfra environment, OpenSSH client tools, `kubectl`, CA certificates, the fixed deploy sources, and the Rust service binary. It runs as UID/GID 65532.

The image includes OCI source and revision labels. It has no Docker `HEALTHCHECK`; Kubernetes owns health checks.

Cargo uses CLI Git for the private Kuri dependency. The release build stages retain BuildKit secret mounts and architecture-specific caches.

## Credential Boundaries

The deployed preview uses one server-held Grafana Editor service-account token for alerting reads and silence creation. An authenticated alert-rule read succeeded. Recording-rule normalization and silence creation remain unverified live.

Pulumi places runtime credentials in the application Secret. It keeps the wrapping-key file in a separate Secret and read-only mount.

The deployed service issues local ES256 access tokens and uses Authentik only for browser identity. Its `/mcp` boundary accepts only locally issued tokens.

PostgreSQL holds generic OAuth/OIDC state and encrypted signing material in the `mcp` schema. Migrations V1-V3 own hosted OAuth, signing, and registration state; V4 adds one-shot OIDC attempts.

The Grafana client sends its token only in the `Authorization` header. It uses fixed datasource, dashboard, render, and alerting API routes with disabled redirects. Alert and recording-rule reads share the fixed provisioning route and partition its response locally. Callers cannot select the origin, token, slug, organization, path, datasource, headers, or method.

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

Pantheon evaluates Kubernetes Service egress after DNAT to control-plane endpoints. The NetworkPolicy therefore allows TCP 6443 only to the Pantheon catalog entry's configured `apiServerEndpointCidrs`; allowing only the Service port 443 does not make the in-cluster API reachable.

Local Pulumi mocks can verify declarations but cannot verify effective cluster authorization or controller behavior. Any Stash seed, preview, apply, credential creation, cluster read, dispatch, rerun, cancellation, or live verification requires exact target-specific authority.

## Kubernetes Access

The Kubernetes runtime, OAuth wiring, container support, Pulumi declarations, and local tests exist in the worktree. They have not been deployed or verified through browser, live-cluster, or effective-RBAC checks. Production remains unapplied.

The [Kubernetes tool specifications](../kubernetes/README.md) own action behavior and limits. The [deployment and RBAC contract](../kubernetes/spec/deployment-rbac.md) owns cluster credentials, exact permissions, process isolation, and operational gates.

Runtime configuration defines one through 32 exact cluster objects and rejects unknown or credential-like fields. The initial catalog represents Pantheon and Romulus with HTTPS API servers on port 6443.

The Tekton deployment kubeconfig serves only as provider bootstrap authority. The runtime uses dedicated reduced credentials for one combined exact-RBAC ServiceAccount in each target cluster.

Each declared runtime ServiceAccount receives cluster-wide fixed reads, exact get-only discovery routes, and only the approved curated writes. It receives no application wildcard, Secret, ConfigMap, arbitrary CRD, pod-log, exec, attach, proxy, port-forward, node-write, force-delete, or general mutation authority. Kubernetes may separately grant broader authenticated discovery through `system:discovery`; the application grant does not remove inherited defaults.

The deployment mounts runtime kubeconfigs separately from provider credentials, application secrets, and OAuth key material. NetworkPolicy derives each cluster egress port from the same validated server URL, using its explicit port or HTTPS default 443, and pairs it with only that cluster's configured endpoint CIDRs.

The reviewed runtime image includes `kubectl`, and server code pins all command behavior. Callers never control an executable, kubeconfig, context, API server, verb, resource path, or output template.

Before any preview apply, an authorized operator must review the exact catalog, ServiceAccounts, ClusterRoles, bindings, mounts, and egress destinations. Live reads, server dry-runs, real mutations, and credential operations require separate target-specific authority.

## Ceph Dashboard Access

The worktree implements the Ceph Dashboard runtime and preview-only Pulumi declarations. Local Rust and Pulumi checks pass. An authorized targeted preview apply seeded four Stashes from shared Dashboard administrator credentials. It updated two provider state records but did not update the application Secret, Deployment, or another Kubernetes resource. No Dashboard account creation, authenticated Ceph read, live Ceph mutation, or preview rollout has occurred.

The [Ceph deployment and access contract](../ceph/spec/deployment-access.md) owns the preview declarations, dedicated per-cluster Dashboard users, Pulumi Stash credential delivery, fixed existing HTTPS destinations, production exclusion, and independent approval gates. The [Ceph tool specifications](../ceph/README.md) own action behavior and mutation safety.

No new inbound service or OAuth route is required. Existing authenticated Ceph Dashboard TLS routes serve the fixed outbound integration destinations. The feature requires no Ceph monitor exposure and no sibling homelab route changes.

Dedicated Dashboard account creation, a full preview Pulumi apply, authenticated live reads, and every individual representative live mutation remain separate gates. Production Ceph declarations remain disabled and unapplied.

## Machine Deploy Access

The worktree implements the machine inventory and deploy runtime. [Machine deploy documentation](../deploys/README.md) owns inventory, bootstrap, host trust, and execution contracts.

`infra/pulumi/index.ts` declares OpenBao resources only when `openbaoEnabled` passes `validateOpenBaoStack`. Preview enables the SSH mount, Ed25519 user CA, user-certificate role, sign-only policy, and Kubernetes auth role. Production disables OpenBao resources and remains unapplied.

The application ServiceAccount receives a projected 600-second JWT with the declared OpenBao audience. The OpenBao role binds that exact ServiceAccount and namespace. Its service token can update only the configured SSH signing route.

The workload mounts the JWT read-only and uses a separate memory-backed deploy credential volume. The image includes a locked offline uv environment, pyinfra, OpenSSH client tools, and the fixed deploy sources.

`HOMELAB_MCP_DEPLOY_*` and `HOMELAB_MCP_OPENBAO_*` values come from `DeployIntegrationConfig` and the `openbao*` Pulumi keys. [SSH trust and OpenBao identity](../deploys/ssh-trust-openbao.md#configuration-authority) lists the exact names and owners.

The `deployMachineSshEgressCidrs` stack key controls machine SSH egress. Preview declares only `172.16.0.0/16`. Pulumi creates one NetworkPolicy rule per CIDR and limits each rule to TCP port 22. Production declares an empty list. An absent or empty key adds no machine SSH rule. These source declarations do not prove cluster enforcement, route reachability, SSH authentication, or a machine deploy.

The platform OpenBao and Tekton configuration is live and canary-verified: `auth/kubernetes` uses in-cluster TokenReview, and the exact `pipelines-as-code/openbao-pulumi-admin-v1` ServiceAccount and matching role issue 30-minute-maximum batch tokens carrying only `openbao-pulumi-admin`. The shared `pipelines-as-code/openbao-kubernetes-login` StepAction is also live and converged from homelab commit `8b98da9`. This repository now declares its consumption, but that pipeline revision has not run.

Only `deploy-preview` selects that ServiceAccount. The shared StepAction alone consumes its projected 600-second, audience-bound JWT and writes the validated batch token atomically to a mode-0400 file on a memory-backed same-pod volume. The normal apply step mounts only that session volume, reads the token into `VAULT_TOKEN` immediately before Pulumi, and unsets the variable and removes the file on exit. The explicit Vault provider does not create a child token. Local applies leave the CI token file absent and can continue to supply `VAULT_TOKEN` directly. Kubernetes deployment authentication remains the separately mounted `tekton-cluster-kubeconfig`. Pulumi, Authentik, and Grafana credentials remain separate. CI receives no administrator SSH credential and runs no pyinfra entrypoint. Operator-local bootstrap uses separately held administrator SSH access.

The application-owned SSH mount, CA, role, sign policy, and runtime Kubernetes role remain declarations in this worktree. No repository pipeline run, preview apply of those resources, workload OpenBao login, certificate issue, SSH authentication, bootstrap, or machine deploy has occurred for this change.

Before any apply, verify the exact `openbaoEndpointCidrs` against current routing. Vault provider `7.11.0` is the exact platform-tested version, but that does not prove this repository's first apply or application resource behavior.

## Preview Pipeline

`.tekton/homelab-mcp-preview.yaml` targets `main` push and incoming events. It defines one preview path and no release path.

The pipeline clones the requested revision and scans Cargo, container, Tekton, and Pulumi inputs for private key patterns. The amd64 and arm64 image builds then run in parallel.

The final `general-ci:latest` step retains Pulumi, Authentik, Grafana, and kubeconfig inputs. Only that deploy task selects the platform OpenBao ServiceAccount; the preceding shared StepAction places its short-lived token on the memory-only session volume before the provider-backed `pulumi up --stack preview --skip-preview`. This source-declared flow has not yet run and does not add preview deployment evidence.

The main preview workflow completed successfully for commit `798dd92` and applied the preview stack.

That run proves image delivery, runtime startup, PostgreSQL-backed readiness, public health/readiness, OAuth metadata, and the unauthenticated MCP Bearer challenge. It does not prove browser login, token issuance or refresh, authenticated MCP calls, or live LogQL behavior.

No release pipeline exists.

## Preview Runtime and Remaining Gate

The preview runtime targets stateless MCP Streamable HTTP revision `2026-07-28` at exact resource `/mcp`.

It requires locally issued tokens with `mcp:use kubernetes:read kubernetes:write inventory:read inventory:write inventory:host-trust deploy:read deploy:run`. It configures DCR, CIMD, and native loopback clients through PostgreSQL-backed OAuth state.

Generic Kuri owns strict OIDC login, callback, one-shot transaction state, ID-token verification, the mapper seam, and hosted continuation. Homelab supplies Authentik configuration and stable issuer-plus-subject mapping.

The current runtime exposes ten read-only actions through `grafana_query`, two image actions through `grafana_render`, and only `silence.create` through separately advertised, operationally consequential `grafana_exec`. The complete eight-scope global set gates all three Grafana tools; no Grafana-specific scope is enforced per action. Their canonical limits, results, and errors are defined by the [Grafana query](../grafana-query/README.md) and [render](../grafana-render/README.md) specifications. Preview commit `798dd92` exposes both `alert-rule.list` and `recording-rule.list` under its explicitly historical authorization state.

Silence creation performs no automatic retry. If it returns `mutation_outcome_unknown`, use `silence.list` to inspect current silences before deciding whether to retry because Grafana may already have applied the request. A silence suppresses matching notifications; it does not stop rule evaluation or delete alert data.

Commit `798dd92` is deployed to preview. An authenticated `alert-rule.list` call returned at least 100 entries. An authenticated `recording-rule.list` call returned `invalid_response` with `limit: 1`. The approved normalization fixes are not deployed or verified live. The rest of the browser OAuth, Grafana, renderer, and Editor operation evidence still requires the layers from the [testing document](../quality/testing.md) and explicit approval for each external action.

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
