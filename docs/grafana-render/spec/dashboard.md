# Render Dashboard

Action `dashboard` performs slugless `GET /render/d/{validated_uid}`. It defaults to 1600 by 1200 pixels, scale 1, dark theme, UTC, and `now-6h` through `now`.

The action uses the shared controls, image validation, MCP result, errors, concurrency, and privacy contract. Its metadata has `render_type: dashboard` and omits `panel_id`.
