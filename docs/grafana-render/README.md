# Grafana Render

`grafana_render` is the fifth progressive MCP tool. It is read-only, idempotent, open-world, and non-destructive. It exposes exactly `help`, `dashboard`, and `panel`; the action names are flat and have no namespace help actions.

| Document | Covers |
| --- | --- |
| [Shared contract](spec/common.md) | Authorization, fixed routes, controls, image validation, results, errors, concurrency, and privacy. |
| [Dashboard](spec/dashboard.md) | Whole-dashboard rendering. |
| [Panel](spec/panel.md) | Single-panel rendering and panel-ID rules. |

Local tests use mock HTTP servers and the pinned Kuri MCP image parser. No live Grafana rendering has been validated. Deployment requires Grafana 13.1.1 and image renderer 5.7.1 deployed separately. The Kuri consumer must include model-visible image support from `6eebdb0` or newer, and the selected model must support image input.
