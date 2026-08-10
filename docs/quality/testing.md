# Testing

## Status

The repository runtime has a passing Rust suite under Rust 1.96. It covers local units and in-process/mock HTTP behavior for configuration, OIDC integration, MCP tool dispatch, Grafana query actions, and cleanup control.

These checks use the exact reviewed Kuri Git pin. The deployed preview still runs the prior health-only image.

Rust 1.95 cannot run the suite because the manifest and Kuri crates require Rust 1.96.

Pulumi has 12 passing mock tests for the declared runtime and deployment contract. Earlier container and health smoke evidence applies to the prior health-only runtime, not the new working-tree binary.

The main pipeline previously ran successfully and applied the preview stack. That evidence covers the foundation deployment only and predates the runtime implementation.

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
| OAuth/MCP | Exact consent, generic hosted-authorization challenge behavior, protocol discovery, tool listing, generated help, filters, calls, and safe JSON-RPC/tool-error boundaries. |
| LogQL/Grafana | Input bounds, normalized result types, fixed routes, redirects, errors, timeout, line truncation, capacity, and permit release against mock HTTP servers. |
| Pulumi policy | Immutable images, HTTPS origins, wrapping-key versions, and stack configuration safety. |
| Pulumi topology | Namespace, backups, CNPG, Authentik, Grafana, Secrets, workload hardening, network, Service, and route. |
| Prior container smoke | The previous health-only image built and ran non-root; this is not evidence for the new binary. |
| Prior preview pipeline | Multi-architecture build, image checks, Pulumi preview/apply, and public health checks for the prior image. |

## Required Test Layers

| Layer | Required coverage before OAuth and LogQL preview approval |
| --- | --- |
| PostgreSQL integration | Apply the real embedded migrations and exercise expiry, atomic single use, replay prevention, refresh rotation, and encrypted signing-key persistence against a disposable database. |
| Hosted OAuth integration | Run complete authorization-code and refresh paths, including local token issuance and validation, beyond local consent, challenge, and mocked generic components. |
| Live Authentik integration | Exercise OIDC discovery, browser login, callback validation, and transaction completion against the configured provider. |
| Kuri-client integration | Exercise DCR, CIMD, native loopback authorization, token refresh, exact resource binding, and `grafana_query` calls. |
| Live Grafana integration | Execute controlled instant and range LogQL through Grafana's datasource proxy without exposing credentials. |
| Container runtime | Build the new image, inspect it for private material, and start it with controlled PostgreSQL, keyring, OAuth/OIDC, and Grafana inputs. |
| Preview end-to-end | After explicit approval, prove browser login, local token issuance, authenticated `/mcp`, and controlled Grafana LogQL on the updated preview image. |

The Rust suite supplies local and mock evidence only. It does not supply live Authentik, live Grafana, container-runtime, or preview end-to-end evidence.

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

## LogQL Contract Coverage

Tests must cover the [Grafana Query specifications](../grafana-query/README.md), including:

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

## Resource and Failure Coverage

Tests must enforce the 30-second timeout, concurrency cap of four, maximum line limit of 5000, and 24-hour range cap.

Capacity tests must prove immediate failure with retryable `capacity_exhausted`. They must prove that a rejected call never reaches Grafana.

Tests must prove cancellation and permit release after success, timeout, transport failure, and malformed response.

Tests must prove that Grafana receives the validated limit. Stream tests must prove deterministic truncation when Grafana overreturns.

## Preview Evidence

The successful main pipeline and applied preview stack provide foundation preview evidence. Public health checks prove only the current health host.

Before OAuth and LogQL preview approval, record the local/mock, PostgreSQL, hosted OAuth, Authentik, Kuri-client, Grafana, and container-runtime results. Then obtain explicit approval for the preview end-to-end check.

Before production approval, record all prior evidence plus image inspection, stack-specific preview review, rotation exercises, and recovery validation. Production remains outside the current target.
