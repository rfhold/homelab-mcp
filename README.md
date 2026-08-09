# homelab-mcp

`homelab-mcp` is a Rust MCP server for authenticated, bounded Grafana LogQL access.

The repository implements hosted OAuth with Authentik browser authentication, stateless MCP, and the progressive `grafana_exec` LogQL action. It uses a reviewed immutable Kuri Git revision, so locked Cargo, Docker, and Tekton builds can resolve the dependency. The deployed preview still runs the prior health-only image.

Implementation entry points are [the Axum process](src/main.rs), [the container build](Dockerfile), [Pulumi](infra/pulumi/), and [the preview pipeline](.tekton/homelab-mcp-preview.yaml).

Start with the [documentation index](docs/README.md) for current behavior, validation evidence, deployment status, and remaining gates.
