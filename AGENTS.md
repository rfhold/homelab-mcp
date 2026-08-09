# Index

| Path | Info |
| --- | --- |
| [src/](src/) | Rust 1.96 Axum health host; MCP behavior does not exist yet. |
| [Dockerfile](Dockerfile) | Multi-stage Rust build and non-root Debian runtime image. |
| [infra/pulumi/](infra/pulumi/) | Preview and production deployment declarations plus mock tests. |
| [.tekton/](.tekton/) | Main-branch preview build and deployment pipeline; no release pipeline exists. |
| [docs/](docs/) | Repository documentation, planned contracts, and the active implementation plan. |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Verified local commands and contribution boundaries. |

# Hints

- Read [docs/README.md](docs/README.md) before work in this repository.
- Distinguish implemented health and deployment foundations from planned MCP behavior.
- Treat Pulumi resources as declarations until an approved `pulumi up` creates them.
- Use Rust 1.96 or the documented container fallback for Rust commands.
- Use `agentic-documentation` for documentation or `AGENTS.md` changes.
- Use `planning-changes` before service, infrastructure, or security implementation.
- Remove the uncommitted active plan when implementation closes or the project is abandoned.
