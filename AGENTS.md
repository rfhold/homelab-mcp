# Index

| Path | Info |
| --- | --- |
| [src/](src/) | Rust 1.96 application composition, hosted OAuth, MCP server, shared services, and integration-owned adapters. |
| [Dockerfile](Dockerfile) | Multi-stage Rust build and non-root Debian runtime image. |
| [infra/pulumi/](infra/pulumi/) | Preview and production deployment declarations plus mock tests. |
| [.tekton/](.tekton/) | Main-branch preview build and deployment pipeline; no release pipeline exists. |
| [.agents/skills/pyinfra/](.agents/skills/pyinfra/) | Project-local pyinfra 3.x authoring guidance and deploy safety routes. |
| [docs/](docs/) | Repository architecture, behavior contracts, operations, quality evidence, and the active implementation plan. |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Verified local commands and contribution boundaries. |

# Hints

- Read [docs/README.md](docs/README.md) before work in this repository.
- Preview runs the OAuth/MCP runtime; distinguish verified basic endpoints from the unverified full browser OAuth and Grafana flow.
- Keep the Kuri `mcp` dependency pinned to a reviewed immutable Git revision before delivery.
- Preview resources exist; production declarations remain unapplied.
- Use Rust 1.96 or the documented container fallback for Rust commands.
- Use `agentic-documentation` for documentation or `AGENTS.md` changes.
- Use `planning-changes` before service, infrastructure, or security implementation.
- Use the project-local `pyinfra` skill for pyinfra deploy authoring or deploy contract changes.
- Remove the uncommitted active plan when implementation closes or the project is abandoned.
