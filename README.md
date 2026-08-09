# homelab-mcp

`homelab-mcp` is a planned Rust MCP server for bounded Grafana LogQL access.

The implemented foundation provides a Rust health host, a container image, Pulumi deployment declarations, and a preview pipeline. It does not provide MCP, OAuth runtime, database use, or Grafana queries.

Implementation entry points are [the Axum process](src/main.rs), [the container build](Dockerfile), [Pulumi](infra/pulumi/), and [the preview pipeline](.tekton/homelab-mcp-preview.yaml).

Start with the [documentation index](docs/README.md) for current behavior, operations, and remaining planned work.
