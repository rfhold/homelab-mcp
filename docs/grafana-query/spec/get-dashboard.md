# Get Dashboard

## Action

`grafana_query` action `dashboard.get` accepts one validated 1-to-40-byte Grafana UID and performs fixed `GET /api/dashboards/uid/{uid}`. The UID grammar permits ASCII letters, digits, `_`, and `-` initially, plus `.` afterward. It cannot contain path, query, fragment, encoding, whitespace, or control syntax.

## Result

The result has mode `get`, result type `dashboard`, and one bounded inventory object. It contains only UID, title, folder UID and title, tags, timezone, version, schema version, at most 100 variables with `name`, optional `label`, and `type`, and recursively flattened panels with `id`, `title`, and `type`.

Non-negative integral numeric and valid string panel IDs normalize to strings accepted by `grafana_render panel`; negative, fractional, unsafe, and overlength IDs reject the inventory response. Nested panel traversal accepts at most 64 nested levels and rejects more than 500 panels. The complete normalized JSON must not exceed 1 MiB.

Panel queries, expressions, datasources, values, options, transformations, field configuration, links, annotations, descriptions, URLs, database IDs, and raw JSON are omitted. Malformed or over-limit inventory returns `invalid_response`; it is not partially returned.
