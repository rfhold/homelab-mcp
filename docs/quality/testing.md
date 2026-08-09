# Testing

## Status

Rust and Pulumi foundation tests exist. MCP, OAuth, persistence, Grafana, and end-to-end tests remain planned.

Rust tests pass with Rust 1.96. Local Rust 1.95 cannot run them because the manifest intentionally requires 1.96.

Pulumi has 12 passing mock tests. The container build and both health endpoints also pass local checks.

## Verified Commands

From the repository root, Rust 1.96 runs:

```bash
cargo test --locked
```

Use this container fallback with an older local Rust toolchain:

```bash
docker run --rm --volume "$PWD:/workspace" --workdir /workspace rust:1.96.0-bookworm cargo test --locked
```

Run Pulumi checks from the repository root:

```bash
cd infra/pulumi
bun install --frozen-lockfile
bun run build
bun test index.test.ts
```

Run the local image and health checks from the repository root:

```bash
docker build --build-arg REVISION=local --tag homelab-mcp:local .
docker run --rm --detach --name homelab-mcp-local --publish 14333:14333 homelab-mcp:local
curl --fail http://127.0.0.1:14333/health
curl --fail http://127.0.0.1:14333/ready
docker stop homelab-mcp-local
```

The endpoint checks prove only unconditional HTTP 200 responses. They do not prove dependency readiness or MCP behavior.

## Current Coverage

| Layer | Implemented coverage |
| --- | --- |
| Rust unit | `/health` and `/ready` return HTTP 200 through the Axum router. |
| Pulumi policy | Immutable images, HTTPS origins, wrapping-key versions, and stack configuration safety. |
| Pulumi topology | Namespace, backups, CNPG, Authentik, Grafana, Secrets, workload hardening, network, Service, route, and safe outputs. |
| Container smoke | Image build, non-root process startup, `/health`, and `/ready`. |

## Planned Test Layers

| Layer | Planned coverage |
| --- | --- |
| Unit | LogQL mode selection, argument validation, limits, token checks, and safe error mapping. |
| HTTP integration | Streamable HTTP negotiation, OAuth metadata, `/mcp` authorization, and Grafana adapter behavior. |
| Database integration | OAuth state, browser-auth state, expiry, replay prevention, and protected signing-key persistence. |
| Infrastructure integration | Provider previews, rendered resources, cluster policy, and stack-specific behavior. |
| End-to-end | Browser authorization, local token issuance, MCP tool call, and Grafana query through a controlled environment. |

## Required Contract Coverage

Tests must cover the [LogQL action specification](../grafana-exec/spec/logql.md), including every mode conflict and resource limit.

Tests must cover the [access invariants](../architecture/access-authentication.md), including direct Authentik token rejection and required `mcp:use` scope enforcement.

## Security Cases

The future suite must verify:

- redirect validation for DCR, CIMD, and loopback clients;
- browser-auth state expiry, binding, and single use;
- issuer, resource, signature, token type, time, and scope validation;
- absence of credentials from logs, MCP results, images, and rendered resources;
- wrapping-key separation from PostgreSQL credentials; and
- BuildKit secret mounts that leave no private Git credential in image layers.

## Resource Cases

The future suite must verify the 30-second timeout, 8 MiB response cap, concurrency cap of four, maximum limit of 5000, and 24-hour range cap.

Tests must also verify cancellation and capacity release after timeout, transport failure, malformed response, and oversized response.

## Release Evidence

Before production approval, record successful Rust tests, integration tests, infrastructure tests, image inspection, and stack-specific preview review.

No stack preview evidence exists yet. A production decision requires approved preview evidence in addition to local checks.
