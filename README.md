# homelab-mcp

`homelab-mcp` is a Rust MCP server for authenticated, bounded homelab integrations.

The repository implements hosted OAuth with Authentik browser authentication and stateless MCP. The [uniform MCP interface](docs/architecture/mcp-interface.md) serves configuration catalogs as resources and live reads or approved mutations through `query`, `create`, `execute`, and `destroy`. Integration-owned clients preserve their typed inputs, normalization, cancellation, and safety boundaries.

Implementation entry points are [the library](src/lib.rs), [the Axum process](src/main.rs), [the container build](Dockerfile), [Pulumi](infra/pulumi/), and [the preview pipeline](.tekton/homelab-mcp-preview.yaml).

Start with the [documentation index](docs/README.md) for current behavior, validation evidence, deployment status, and remaining gates.
