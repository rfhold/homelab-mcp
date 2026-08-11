# Grafana Render Shared Contract

## Request Boundary

The existing `mcp:use` scope and shared Grafana Editor token authorize rendering. Requests use only the configured Grafana origin, Bearer header, fixed GET routes, and server-built parameters. Redirects and environment proxies are disabled. Callers cannot set an origin, slug, organization ID, header, method, callback, URL, or arbitrary parameter.

Both actions accept validated width 320 through 2000, height 200 through 2000, scale 1 through 2, light or dark theme, IANA timezone, and at most 20 sorted template variables. Variable names are 1 through 64 bytes and values at most 1024 bytes. Effective `width * height * scale^2` cannot exceed 4,000,000 pixels.

The default range is `now-6h` through `now`. `from` and `to` are each limited to 64 UTF-8 bytes before parsing. Relative ranges support positive seconds, minutes, hours, or days through 24 hours and require `to=now`. Absolute RFC3339 endpoints normalize to epoch milliseconds and span at most 24 hours. IANA timezone names are limited to 32 UTF-8 bytes before lookup. Generated schemas expose matching maximum lengths.

Query order is optional `panelId`, `width`, `height`, `scale`, `theme`, `tz`, `from`, `to`, fixed `timeout=20`, then sorted `var-*` entries. There are no retries.

## Response Boundary

Only HTTP 200 is success. The response media type must be `image/png`, case-insensitively, while parameters are allowed. The body is streamed through the inclusive 4 MiB limit, must contain at least eight bytes, and must begin with the PNG signature. Body-read failures, wrong MIME, oversize bodies, and bad signatures return `render_invalid_response`; error bodies are never read.

The complete render timeout is 25 seconds. Four global immediate permits remain shared by every Grafana action, and rendering adds two immediate permits. Both permits are released on success, failure, timeout, drop, or cancellation.

| Code | Condition | Retryable |
| --- | --- | --- |
| `invalid_arguments` | Input or encoded URL validation fails. | `false` |
| `capacity_exhausted` | A global or render permit is unavailable. | `true` |
| `render_timeout` | The complete render exceeds 25 seconds. | `true` |
| `grafana_unauthorized` | Grafana returns 401 or 403. | `false` |
| `render_rejected` | Grafana returns another 4xx. | `false` |
| `upstream_unavailable` | Transport failure, 3xx, or 5xx. | `true` |
| `render_invalid_response` | Successful response MIME, body read, size, or signature is invalid. | `false` |

## MCP Result and Privacy

Success returns short text and a raw standard-padded Base64 image block with `type: image` and `mimeType: image/png`. Object `structuredContent` contains `render_type`, `uid`, optional `panel_id`, normalized `from` and `to`, dimensions, scale, theme, timezone, decoded byte count, and lowercase SHA-256.

Base64, template variable names and values, origin, URL, authorization, and headers are absent from metadata. Image-byte `Debug` output is redacted. A progressive `filter` changes only `structuredContent` and synchronized text; it preserves image content.

Telemetry records only fixed action `dashboard` or `panel`, mode `render`, destination `grafana_rendering`, and allowlisted outcomes. It never records UID, panel ID, range, timezone, dimensions, variables, digest, URL, body, image, token, headers, or upstream error body.

MCP cancellation follows the shared read-only `request cancelled` boundary. Dropping the in-flight render still releases both permits.
