# Testing

## Status

The repository runtime has a passing Rust suite under Rust 1.96. It covers local units and in-process/mock HTTP behavior for configuration, OIDC integration, MCP tool dispatch, Grafana read and silence actions, and cleanup control.

These checks use the exact reviewed Kuri Git pin. Preview runs the authenticated runtime.

Rust 1.95 cannot run the suite because the manifest and Kuri crates require Rust 1.96.

Pulumi has 12 passing mock tests for the declared runtime and deployment contract. The current container built and deployed successfully to preview.

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
| OAuth/MCP | Exact consent, generic hosted-authorization challenge behavior, protocol discovery, two-tool listing and annotations, generated help, filters, calls, and safe JSON-RPC/tool-error boundaries. |
| Grafana actions | Datasource queries, alert-rule, alert-instance, and silence reads, silence creation, input bounds, normalized results, fixed routes, redirects, read and mutation errors, timeout, capacity, and permit release against mock HTTP servers. |
| Pulumi policy | Immutable images, HTTPS origins, wrapping-key versions, Editor service-account declaration, and stack configuration safety. |
| Pulumi topology | Namespace, backups, CNPG, Authentik, Grafana, Secrets, workload hardening, network, Service, and route. |
| Current container runtime | Multi-architecture image delivery succeeded; the preview pod is ready with zero restarts. |
| Current preview pipeline | Seven tasks passed, Pulumi applied preview, health/readiness return 200, OAuth metadata is live, and unauthenticated `/mcp` returns the required Bearer challenge. |

## Required Test Layers

| Layer | Remaining coverage before full OAuth and Grafana preview acceptance |
| --- | --- |
| PostgreSQL integration | Apply the real embedded migrations and exercise expiry, atomic single use, replay prevention, refresh rotation, and encrypted signing-key persistence against a disposable database. |
| Hosted OAuth integration | Run complete authorization-code and refresh paths, including local token issuance and validation, beyond local consent, challenge, and mocked generic components. |
| Live Authentik integration | Exercise OIDC discovery, browser login, callback validation, and transaction completion against the configured provider. |
| Kuri-client integration | Exercise DCR, CIMD, native loopback authorization, token refresh, exact resource binding, and calls to both MCP tools. |
| Live Grafana integration | Exercise controlled datasource reads, alert-rule, alert-instance, and silence reads, and silence creation without exposing credentials. Verify that Editor permits only the intended operation. |
| Container runtime | Basic deployed startup is verified; complete the full browser OAuth and Grafana path in the deployed container. |
| Preview end-to-end | Prove browser login, local token issuance and refresh, authenticated `/mcp`, and controlled Grafana datasource and alerting reads and silence creation on preview. |

The Rust suite supplies local and mock evidence. Deployed evidence additionally covers startup, readiness, metadata, and challenge behavior, but not browser OAuth, authenticated MCP, live alert APIs, or Editor permission operation. No deployment or live operation occurred for the alerting revision.

The Tekton feature has worktree implementation, Rust unit and MCP discovery tests, and passing Pulumi declaration tests. It has no deployment or live evidence. Existing preview evidence does not cover `tekton_query`, `tekton_exec`, Forgejo, PAC, effective Kubernetes RBAC, or task logs.

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
- optional normalized statistics, short unfiltered summaries, and filtered output wrapping;
- every stable semantic error code and retryable value; and
- JSON-RPC errors for malformed protocol, tool shape, action, and filter requests.

Alerting tests must additionally cover:

- both tool annotations, seven query actions, and the single exec action;
- the all-`mcp:use` authorization boundary and rejection of actions sent to the wrong tool;
- alert-rule limits, fixed provisioning route, bounded summaries, and the documented conservative URL-field exclusions;
- alert-instance matcher grammar and byte limits, repeated server-built filters, list limits, status booleans, and safe normalized maps;
- silence-list state and limit bounds, fixed route without caller-controlled parameters, filtering before truncation, strict response validation, and normalized recovery fields;
- required silence matchers, duration and comment bounds, immediate start, fixed `createdBy`, and exact three-field success output;
- no automatic mutation retry, safe `mutation_rejected`, and non-retryable `mutation_outcome_unknown` for ambiguous post-dispatch failures; and
- action-specific result allowlists, with normalized matchers and comments exposed only by `silence.list`, and exclusion of sensitive action data from errors, logs, spans, and metrics.

## Resource and Failure Coverage

Tests must enforce the shared 30-second timeout, concurrency cap of four, 8192-byte URL cap, and 4 MiB response cap, plus action-specific query and result limits.

Capacity tests must prove immediate failure with retryable `capacity_exhausted`. They must prove that a rejected call never reaches Grafana.

Tests must prove cancellation and permit release after success, timeout, transport failure, and malformed response. MCP cancellation must retain generic request-cancelled behavior for reads; cancellation of a dispatched silence POST must return safe, non-retryable `mutation_outcome_unknown` and direct callers to inspect current silences.

For actions whose upstream API accepts a limit, tests must prove that Grafana receives the validated limit. Actions with a local result limit must prove deterministic truncation when Grafana overreturns; `silence.list` intentionally applies its limit only after local state filtering.

## Tekton Contract Coverage

Tests must cover the [Tekton tool specifications](../tekton/README.md), including:

- generated help, schemas, filters, action separation, and exact MCP annotations;
- all-`mcp:use` authorization, including access by every current MCP principal;
- PAC `Repository` authority in fixed namespace `pipelines-as-code`;
- exclusion of invalid repository URLs and normalization to the fixed Forgejo origin;
- direct root `.tekton/*.yaml` and `.tekton/*.yml` discovery only;
- fixed fanout, file, byte, YAML document, result, step, tail, and log-byte limits;
- partial workflow-discovery failures and every `PipelineRun` definition and event;
- triggerable status only for exact `incoming` events;
- namespace-qualified run and task IDs, ownership validation, reverse chronology, and output allowlists;
- task-log truncation metadata, MCP-held secret redaction, and no telemetry log content;
- dispatch validation, fixed PAC POST `/incoming`, caller-control rejection, and 2xx acceptance semantics;
- safe rerun replay of the prior branch and parameters;
- active owned-run checks and a `spec.status=Cancelled`-only patch;
- no automatic retries and non-retryable `mutation_outcome_unknown` after ambiguous sends; and
- exclusion of secrets, raw objects, internal routes, and upstream bodies from MCP, logs, traces, metrics, and errors.

Pulumi mock tests must cover separate env-backed Stashes, environment seed names, application Secret projection, the dedicated ServiceAccount, explicit token projection, namespace Role rules, and absence of Secret or cluster-wide permissions.

Live preview evidence requires separate approval for each target and action. It must verify effective RBAC, PAC repository mapping, Forgejo reads, bounded logs, dispatch acceptance, rerun behavior, cancellation requests, and uncertain mutation recovery.

## Preview Evidence

The successful main pipeline and applied preview stack provide foundation preview evidence. Public health checks prove only the current health host.

Before full OAuth and Grafana preview approval, record the local/mock, PostgreSQL, hosted OAuth, Authentik, Kuri-client, Grafana, and container-runtime results. This must include live alert API behavior and Editor permission operation. Then obtain explicit approval for the preview end-to-end check and any silence creation.

Before production approval, record all prior evidence plus image inspection, stack-specific preview review, rotation exercises, and recovery validation. Production remains outside the current target.
