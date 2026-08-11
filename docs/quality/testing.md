# Testing

## Status

The repository runtime has 150 passing Rust tests under Rust 1.96: 148 library tests and two binary tests. It covers local units and in-process/mock HTTP behavior for configuration, OIDC integration, MCP tool dispatch, Grafana query, rendering, silence actions, Tekton actions, Kubernetes actions, and cleanup control.

These checks use the exact reviewed Kuri Git pin. Preview runs the authenticated runtime.

Rust 1.95 cannot run the suite because the manifest and Kuri crates require Rust 1.96.

Pulumi has 20 passing mock tests for the declared runtime and deployment contract. The current container built and deployed successfully to preview; the Kubernetes worktree revision has not been deployed.

PipelineRun `homelab-mcp-preview-tfd8k` completed all seven tasks and applied commit `4f2e192` to preview.

## Verified Commands

Run the full local validation set from the repository root:

```bash
cargo +1.96.0 fmt --all -- --check
cargo +1.96.0 check --locked --all-targets --all-features
cargo +1.96.0 clippy --locked --all-targets --all-features -- -D warnings
cargo +1.96.0 test --locked --all-features
```

Coordinator evidence records 111 passing standard tests for Kuri `mcp` with all features. Both normally ignored Docker-backed PostgreSQL tests also pass when run explicitly.

The Docker runtime build and Tekton image-build tasks can resolve the reviewed Git pin. Rust tests run directly through Cargo rather than through a Docker target or a dedicated Tekton test task.

Run Pulumi checks from the repository root:

```bash
cd infra/pulumi
bun install --frozen-lockfile
bun run build
bun test index.test.ts
```

Do not use the old standalone `docker run` smoke sequence for the new binary. Startup requires PostgreSQL, a mounted OAuth wrapping keyring, Authentik OIDC and local OAuth configuration, and Grafana credentials. A meaningful process smoke test needs those controlled dependencies; a full hosted flow needs stronger evidence still.

## Current Coverage

| Layer | Implemented coverage |
| --- | --- |
| Rust local suite | The library and binary tests pass with `cargo +1.96.0 test --locked --all-features`; no tests are ignored. |
| Generic Kuri `mcp` | 111 standard all-feature tests pass; both normally ignored Docker-backed PostgreSQL tests also pass when run explicitly. |
| Configuration and host | Keyring parsing, secure Grafana origin validation, and health/readiness state behavior. |
| Generic OIDC integration | Strict callback use, hosted continuation, and stable issuer-plus-subject mapping through generic seams. |
| OAuth/MCP | Exact consent, generic hosted-authorization challenge behavior, protocol discovery, seven-tool listing and annotations, generated help, filters, image content parsing, calls, and safe JSON-RPC/tool-error boundaries. |
| Grafana actions | Datasource queries, dashboard inventory and PNG rendering, alerting reads and silence creation, bounds, normalization, fixed routes, redirects, semantic errors, timeout, capacity, and permit release against mock HTTP servers. |
| Kubernetes actions | Exact typed action schemas, all 36 resource kinds, namespace scope, fixed API paths and mutations, normalization, limits, safe errors, process supervision, and uncertain mutation outcomes. |
| Pulumi policy | Immutable images, HTTPS origins, wrapping-key versions, strict normalized Kubernetes cluster configuration and ports, Editor service-account declaration, and stack configuration safety. |
| Pulumi topology | Namespace, backups, CNPG, Authentik, Grafana, Kubernetes identities and exact RBAC, Secrets, workload hardening, per-cluster egress, Service, and route. |
| Current container runtime | Multi-architecture image delivery succeeded; the preview pod is ready with zero restarts. |
| Current preview pipeline | Seven tasks passed, Pulumi applied preview, health/readiness return 200, OAuth metadata is live, and unauthenticated `/mcp` returns the required Bearer challenge. |

## Required Test Layers

| Layer | Remaining coverage before full OAuth and Grafana preview acceptance |
| --- | --- |
| PostgreSQL integration | Apply the real embedded migrations and exercise expiry, atomic single use, replay prevention, refresh rotation, and encrypted signing-key persistence against a disposable database. |
| Hosted OAuth integration | Run complete authorization-code and refresh paths, including local token issuance and validation, beyond local consent, challenge, and mocked generic components. |
| Live Authentik integration | Exercise OIDC discovery, browser login, callback validation, and transaction completion against the configured provider. |
| Kuri-client integration | Exercise DCR, CIMD, native loopback authorization, token refresh, exact resource binding, and calls to all advertised MCP tools. |
| Live Grafana integration | Exercise controlled datasource and dashboard reads, rendering, alerting reads, and silence creation without exposing credentials. Verify the renderer prerequisites and intended Editor operations. |
| Container runtime | Basic deployed startup is verified; complete the full browser OAuth and Grafana path in the deployed container. |
| Preview end-to-end | Prove browser login, local token issuance and refresh, authenticated `/mcp`, and controlled Grafana datasource and alerting reads and silence creation on preview. |

The Rust suite supplies local and mock evidence. Deployed evidence additionally covers startup, readiness, metadata, and challenge behavior, but not browser OAuth, authenticated MCP, live alert APIs, or Editor permission operation. No deployment or live operation occurred for the alerting revision.

The Tekton feature has worktree implementation, Rust unit and MCP discovery tests, and passing Pulumi declaration tests. It has no deployment or live evidence. Existing preview evidence does not cover `tekton_query`, `tekton_exec`, Forgejo, PAC, effective Kubernetes RBAC, or task logs.

The Kubernetes feature has worktree implementation, Rust unit and MCP schema tests, and passing Pulumi policy and topology tests. Existing preview evidence does not cover its OAuth scope set, runtime kubeconfigs, cluster API reachability, effective RBAC, reads, dry-runs, or mutations.

Preview end-to-end checks require explicit target authority. Production checks and production deployment remain excluded.

## OAuth Contract Coverage

Tests must cover the [access contract](../architecture/access-authentication.md), including:

- exact `/mcp` resource and issuer binding;
- MCP revision `2026-07-28` and stateless requests;
- absence of MCP session identifiers and protocol session state;
- Bearer challenges for absent, invalid, and insufficient-scope tokens;
- direct Authentik access-token and ID-token rejection;
- required `mcp:use` enforcement;
- DCR, hardened CIMD, and native loopback redirects;
- authorization code PKCE S256 and redirect binding;
- Authentik OIDC state, nonce, PKCE, signature, issuer, and audience checks;
- generic OIDC transaction expiry, digest-only state and correlation, secure cookies, and atomic single use;
- consent auto-approval only after authentication for the configured resource and scope;
- authorization-code replay and refresh-family rotation behavior; and
- encrypted ES256 signing material with separate database and wrapping-key secrets.

Negative tests must verify that errors, logs, redirects, traces, and MCP content contain no secret material.

## Grafana Contract Coverage

Tests must cover the [Grafana tool specifications](../grafana-query/README.md), including:

- macro-generated `help`, nested `input`, schema, and optional jq-compatible `filter` behavior;
- every instant and range field combination;
- non-empty queries and RFC3339 timestamps;
- `forward` and `backward`, including the range default;
- default and maximum limits plus every pre-request validation rule;
- fixed datasource UID `loki` and Grafana proxy-only routing;
- exact instant-query and range-query proxy paths;
- Authorization-header-only token use and disabled redirects;
- normalized `streams`, `matrix`, `vector`, and `scalar` results;
- deterministic aggregate stream-entry truncation to the validated line limit;
- optional normalized statistics and synchronized complete unfiltered JSON text and structured content;
- direct successful semantic filter output, synchronized compact text, and legacy `{ "result": ... }` wrapping for generated help and other JSON actions;
- every stable semantic error code and retryable value; and
- JSON-RPC errors for malformed protocol, tool shape, action, and filter requests.

Dashboard and render tests additionally cover:

- `dashboard.list` and `dashboard.get` schemas, exact routes, deterministic query order, bounded normalization, recursive panel flattening, numeric and string panel IDs, strict variable/panel/output caps, and omission of raw dashboard internals;
- exactly flat `grafana_render` actions `dashboard` and `panel`, slugless routes, server-owned parameters, render-only capacity, complete timeout, and permit release;
- exact status mapping, PNG MIME parameters, streaming size and signature validation, body-read safety, lowercase SHA-256, redacted image `Debug`, and standard-padded Base64;
- text plus typed MCP image parsing through the pinned Kuri revision, structured metadata omissions, and filters that preserve image content; and
- fixed render telemetry labels excluding UIDs, panel IDs, ranges, dimensions, timezones, variables, digests, URLs, bodies, images, and credentials.

Alerting tests must additionally cover:

- all three Grafana tool annotations, nine query actions, two render actions, and the single exec action;
- the all-`mcp:use` authorization boundary and rejection of actions sent to the wrong tool;
- alert-rule limits, fixed provisioning route, bounded summaries, and the documented conservative URL-field exclusions;
- alert-instance matcher grammar and byte limits, repeated server-built filters, list limits, status booleans, and safe normalized maps;
- silence-list state and limit bounds, fixed route without caller-controlled parameters, filtering before truncation, strict response validation, and normalized recovery fields;
- required silence matchers, duration and comment bounds, immediate start, fixed `createdBy`, and exact three-field success output;
- no automatic mutation retry, safe `mutation_rejected`, and non-retryable `mutation_outcome_unknown` for ambiguous post-dispatch failures; and
- action-specific result allowlists, with normalized matchers and comments exposed only by `silence.list`, and exclusion of sensitive action data from errors, logs, spans, and metrics.

## Resource and Failure Coverage

Tests must enforce the shared 30-second timeout, concurrency cap of four, 8192-byte URL cap, and 4 MiB response cap, plus action-specific query and result limits. Rendering additionally enforces a 25-second complete timeout and two immediate render permits while retaining the global permit.

Capacity tests must prove immediate failure with retryable `capacity_exhausted`. They must prove that a rejected call never reaches Grafana.

Tests must prove cancellation and permit release after success, timeout, transport failure, and malformed response. MCP cancellation must retain generic request-cancelled behavior for reads; cancellation of a dispatched silence POST must return safe, non-retryable `mutation_outcome_unknown` and direct callers to inspect current silences.

For actions whose upstream API accepts a limit, tests must prove that Grafana receives the validated limit. Actions with a local result limit must prove deterministic truncation when Grafana overreturns; `silence.list` intentionally applies its limit only after local state filtering.

## Tekton Contract Coverage

Tests must cover the [Tekton tool specifications](../tekton/README.md), including:

- generated help, schemas, filters, action separation, and exact MCP annotations;
- all-`mcp:use` authorization, including access by every current MCP principal;
- PAC `Repository` authority in fixed namespace `pipelines-as-code`;
- canonical `org/repo` keys from valid fixed-origin Forgejo URLs for all repository selectors and relationships, including safe unreserved percent-decoding and encoded-separator rejection;
- internal-only PAC custom-resource names, no legacy aliases, and fail-closed duplicate canonical keys;
- direct root `.tekton/*.yaml` and `.tekton/*.yml` discovery only;
- fixed fanout, file, byte, YAML document, result, step, tail, and log-byte limits;
- partial workflow-discovery failures and every `PipelineRun` definition and event;
- triggerable status only for exact `incoming` events;
- workflow ID rotation from canonical repository hash input and required client rediscovery;
- namespace-qualified run and task IDs, ownership validation, reverse chronology, and output allowlists;
- exact workflow, status, and revision run filters plus equivalent `main` and `refs/heads/main` branch labels;
- newest matching `run.list` lookup with `limit: 1` and separate source, result, and aggregate truncation fields;
- bounded `run.wait` timeout, fixed polling, terminal and deadline results, cancellation, and separate waiter capacity;
- wait-time upstream permit release, output-time ownership revalidation, and exclusion of task details and logs;
- task-log truncation metadata, MCP-held secret redaction, and no telemetry log content;
- dispatch validation, fixed PAC POST `/incoming`, caller-control rejection, and 2xx acceptance semantics;
- safe rerun replay of the prior branch and parameters;
- active owned-run checks and a `spec.status=Cancelled`-only patch;
- no automatic retries and non-retryable `mutation_outcome_unknown` after ambiguous sends;
- complete unfiltered normalized JSON in text content synchronized with `structuredContent`, including identities and mutation acceptance details needed by follow-up actions; and
- exclusion of secrets, raw objects, internal routes, and upstream bodies from MCP, logs, traces, metrics, and errors.

Pulumi mock tests must cover separate env-backed Stashes, environment seed names, application Secret projection, the dedicated ServiceAccount, explicit token projection, namespace Role rules, and absence of Secret or cluster-wide permissions.

Live preview evidence requires separate approval for each target and action. It must verify effective RBAC, PAC repository mapping, Forgejo reads, bounded logs, dispatch acceptance, rerun behavior, cancellation requests, and uncertain mutation recovery.

## Kubernetes Contract Coverage

The [Kubernetes tool specifications](../kubernetes/README.md) define locally implemented behavior. Rust and Pulumi tests cover the runtime and declarations. Deployment, browser, live-cluster, and effective-RBAC evidence remain pending.

Tests must cover:

- progressive help, typed action schemas, optional jq-compatible filters, action separation, and exact MCP annotations;
- the global `mcp:use kubernetes:read kubernetes:write` requirement for `/mcp`, automatic Kuri requests, and current-grant reauthorization;
- proof that OAuth scopes do not enforce per-action access;
- a unique one-through-32 cluster catalog, exact cluster selection, and initial Pantheon and Romulus entries;
- rejection of caller-controlled executables, kubeconfigs, contexts, API servers, arguments, verbs, resources, API paths, selectors, and output templates;
- `cluster_list`, `capability_list`, `resource_list`, and `resource_get` validation and normalized output, including schema-visible namespace requirements and disjoint kind subsets;
- every approved built-in and fixed platform resource kind, API mapping, namespaced rule, and unsupported capability result;
- exact label maps with at most eight entries and rejection of arbitrary selector syntax;
- five-page, 500-inspected, and 100-returned list ceilings with separate source, result, and aggregate truncation fields;
- conditions capped at 20, Event messages capped at 1,024 UTF-8 bytes each, and Event text capped at 32 KiB per result;
- normalized allowlists with no raw objects, Secrets, ConfigMaps, arbitrary labels, arbitrary annotations, or arbitrary custom resources;
- two concurrent `kubectl` processes, an outer deadline of at most 30 seconds, 4 MiB stdout, 32 KiB stderr, cancellation, child termination, and permit release;
- safe semantic errors with no command, path, credential, API origin, stdout, stderr, or raw-object disclosure, including unknown outcomes for every non-success mutation process result after spawn;
- exact-object restart for Deployment, StatefulSet, and DaemonSet only;
- exact-object scale for Deployment and StatefulSet only, with replicas from 0 through 1,000;
- exact CronJob suspend or resume state, server-named CronJob trigger, and ordinary exact Pod deletion;
- server dry-run where supported, one process attempt, no retry, safe rejection, and non-retryable `mutation_outcome_unknown` after launch ambiguity; and
- every exclusion in the [shared contract](../kubernetes/spec/common.md#exclusions).

Deployment tests verify dedicated runtime kubeconfigs, strict normalized cluster configuration, per-server NetworkPolicy ports, and one combined exact-RBAC ServiceAccount per target cluster. They prove that the Tekton provider kubeconfig never reaches the runtime.

RBAC declaration tests verify fixed cluster-wide reads, exact curated writes, exact get-only discovery routes, and no application wildcard permissions. Standard Kubernetes may independently grant broader authenticated discovery through `system:discovery`; live effective-RBAC checks must account for inherited defaults. Representative denials must cover Secrets, ConfigMaps, arbitrary CRDs, pod logs, exec, attach, proxy, port forwarding, node writes, force deletion, and general mutation.

Preview evidence requires exact target-specific approval. It must verify identity, effective RBAC, API reachability, catalog behavior, representative reads, denied exclusions, server dry-run, controlled accepted mutations, and uncertain-outcome recovery.

Current preview evidence does not cover `kubernetes_query`, `kubernetes_exec`, the expanded OAuth scope set, runtime kubeconfigs, effective Kubernetes RBAC, or any Kubernetes action. Production remains unapplied and outside current verification.

## Preview Evidence

The successful main pipeline and applied preview stack provide foundation preview evidence. Public health checks prove only the current health host.

Before full OAuth and Grafana preview approval, record the local/mock, PostgreSQL, hosted OAuth, Authentik, Kuri-client, Grafana, and container-runtime results. This must include live alert API behavior and Editor permission operation. Then obtain explicit approval for the preview end-to-end check and any silence creation.

Before production approval, record all prior evidence plus image inspection, stack-specific preview review, rotation exercises, and recovery validation. Production remains outside the current target.
