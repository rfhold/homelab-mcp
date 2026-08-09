# Contributing

The repository implements a Rust health host, container build, Pulumi declarations, and a preview pipeline. MCP and OAuth runtime behavior remain planned.

Use [docs/README.md](docs/README.md) to locate canonical contracts and planned behavior.

## Verified Local Commands

Rust commands require Rust 1.96 because `Cargo.toml` sets `rust-version = "1.96"`. Rust 1.95 rejects the project before tests run.

Run Rust tests with Rust 1.96:

```bash
cargo test --locked
```

Use the verified container fallback when the local toolchain is older:

```bash
docker run --rm --volume "$PWD:/workspace" --workdir /workspace rust:1.96.0-bookworm cargo test --locked
```

Run Pulumi type checks and the 12 mock tests:

```bash
cd infra/pulumi
bun install --frozen-lockfile
bun run build
bun test index.test.ts
```

Build and check the local container:

```bash
docker build --build-arg REVISION=local --tag homelab-mcp:local .
docker run --rm --detach --name homelab-mcp-local --publish 14333:14333 homelab-mcp:local
curl --fail http://127.0.0.1:14333/health
curl --fail http://127.0.0.1:14333/ready
docker stop homelab-mcp-local
```

See [the testing guide](docs/quality/testing.md) for current coverage and planned validation.

For documentation-only changes:

1. Keep current state separate from planned behavior.
2. Update the owning document and its domain index.
3. Run `git diff --check`.
4. Verify that every relative Markdown link resolves.
5. Verify that every populated domain under `docs/` has a `README.md` index.

Do not commit, push, preview, deploy, or mutate an external system without explicit authority.

Pulumi stacks `preview` and `prod` are initialized with zero resources. Do not run `pulumi preview` or `pulumi up` without explicit target-specific authority.
