# Render Panel

Action `panel` performs slugless `GET /render/d-solo/{validated_uid}` and emits validated `panelId` first in the query. It defaults to 1000 by 500 pixels, scale 1, dark theme, UTC, and `now-6h` through `now`.

Panel IDs are strings of 1 through 64 ASCII letters, digits, `_`, `.`, or `-`, including current identifiers such as `panel-13`. Dashboard inventory normalizes both numeric and string IDs to this string form. The action uses the shared controls, image validation, errors, concurrency, and privacy contract; metadata has `render_type: panel` and includes `panel_id`.
