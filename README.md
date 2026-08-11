# homelab-mcp

`homelab-mcp` is a Rust MCP server for authenticated, bounded homelab integrations.

The repository implements hosted OAuth with Authentik browser authentication, stateless MCP, the read-only `grafana_query` and `grafana_render` tools, and the operationally consequential `grafana_exec` tool. Grafana actions cover datasource queries, alerting, dashboard inventory, bounded PNG rendering, and bounded silence creation. The application uses a concrete services composition root and integration-owned clients, actions, errors, and telemetry so additional integrations can be added without coupling MCP dispatch to their SDKs.

Implementation entry points are [the library](src/lib.rs), [the Axum process](src/main.rs), [the container build](Dockerfile), [Pulumi](infra/pulumi/), and [the preview pipeline](.tekton/homelab-mcp-preview.yaml).

Start with the [documentation index](docs/README.md) for current behavior, validation evidence, deployment status, and remaining gates.
