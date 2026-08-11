# List Dashboards

## Action

`grafana_query` action `dashboard.list` performs fixed `GET /api/search?type=dash-db` dashboard discovery. Optional inputs are a non-empty, control-free query of at most 256 UTF-8 bytes; at most 20 control-free tags of 128 bytes each; page 1 through 10000; and limit 1 through 100. Defaults are page 1 and limit 50.

Tags are sorted and deduplicated. Query parameters are emitted deterministically as `type`, optional `query`, repeated `tag`, `page`, and `limit`. The caller cannot supply folders, kinds, routes, methods, headers, origin, or arbitrary parameters.

## Result

The result has mode `list`, result type `dashboards`, and upstream-ordered dashboard summaries containing only `uid`, `title`, `folder_uid`, `folder_title`, and `tags`. Folder fields may be null. Database IDs, URLs, URIs, slugs, internal sort data, and raw JSON are omitted.

The shared 4 MiB upstream body, URL, timeout, concurrency, authorization, error, filtering, and telemetry contracts apply.
