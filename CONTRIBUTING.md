# Contributing

The repository implements hosted OAuth, authenticated MCP, and the progressive Grafana runtime. The generic Kuri `mcp` dependency uses a reviewed immutable Git revision.

Preview runs this runtime and has verified health, readiness, OAuth metadata, and the unauthenticated MCP Bearer challenge. Full browser OAuth and live Grafana queries remain unverified. Production remains excluded.

Use [docs/README.md](docs/README.md) to locate canonical contracts and current validation boundaries.

Use [machine deploy documentation](docs/deploys/README.md) for inventory, bootstrap, SSH trust, OpenBao identity, and deploy safety.

## Local Commands

Rust commands require Rust 1.96 because `Cargo.toml` sets `rust-version = "1.96"`. Rust 1.95 rejects the project before tests run.

Run formatting, check, Clippy, and tests with Rust 1.96:

```bash
cargo +1.96.0 fmt --all -- --check
cargo +1.96.0 check --locked --all-targets --all-features
cargo +1.96.0 clippy --locked --all-targets --all-features -- -D warnings
cargo +1.96.0 test --locked --all-features
```

These commands pass against the exact Kuri Git pin.

If the installed Cargo does not resolve the toolchain shorthand, use the environment-compatible form:

```bash
rustup run 1.96.0 cargo fmt --all -- --check
rustup run 1.96.0 cargo check --locked --all-targets --all-features
rustup run 1.96.0 cargo clippy --locked --all-targets --all-features -- -D warnings
rustup run 1.96.0 cargo test --locked --all-features
```

Build the private-dependency runtime image with BuildKit secret handling:

```bash
DOCKER_BUILDKIT=1 docker build \
  --build-arg REVISION=local \
  --secret id=gitconfig,src="$HOME/.gitconfig" \
  --secret id=git-credentials,src="$HOME/.git-credentials" \
  --tag homelab-mcp:local \
  .
```

The secret source files must exist. BuildKit mounts them only for Cargo's fetch/build step.

Run Pulumi type checks and mock tests:

```bash
cd infra/pulumi
bun install --frozen-lockfile
bun run build
bun test index.test.ts
```

Run deploy project checks without contacting a machine:

```bash
uv lock --check
PYTHONDONTWRITEBYTECODE=1 uv run --locked python -m unittest discover -s deploys/tests
docker compose -f deploys/fixtures/compose.yaml config --quiet
```

The Compose command validates fixture configuration only. It does not pull or run images.

Do not run `bootstrap-homelab` during routine validation. It mutates sudo and sshd configuration on one exact target. Follow [the operator procedure](docs/deploys/deploy-workflow.md#operator-bootstrap) only with target-specific authority.

Do not treat a standalone `docker run` as a runtime smoke test. Startup requires PostgreSQL, an OAuth wrapping keyring, OIDC configuration, local OAuth settings, and Grafana credentials. See [the testing guide](docs/quality/testing.md) for current coverage.

For documentation-only changes:

1. Keep current state separate from planned behavior.
2. Update the owning document and its domain index.
3. Run `git diff --check`.
4. Verify that every relative Markdown link resolves.
5. Verify that every populated domain under `docs/` has a `README.md` index.

Do not commit, push, preview, deploy, or mutate an external system without explicit authority.

The `preview` stack is deployed by the main pipeline. The `prod` stack is initialized with zero resources. Do not run `pulumi preview` or `pulumi up` without explicit target-specific authority.
