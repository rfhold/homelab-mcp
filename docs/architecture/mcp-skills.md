# MCP Skills

The authenticated server retains eleven tools and 57 domain actions. Its reviewed Kuri MCP dependency removes generated `help` and `help.<namespace>` actions as a hard cutover; there is no compatibility shim. `tools/list` remains the authority for exact input schemas and annotations. Domain action inputs, semantic outputs, jq-compatible filters, images, authorization, cancellation context, and progress heartbeats remain unchanged.

## Catalog

[`src/mcp/skills.rs`](../../src/mcp/skills.rs) constructs an immutable `SkillCatalog` from explicitly enumerated `include_bytes!` assets under [`src/skills/`](../../src/skills/). Construction is fallible and router initialization propagates validation errors rather than advertising a partial catalog. `HomelabMcp` owns `Arc<SkillCatalog>` and registers `skills = self.catalog` with the server macro. There are no directory scans, runtime filesystem reads, generated help wrappers, or new tools.

| Skill | Tool domains |
| --- | --- |
| `investigate-observability` | Grafana query, render, and alert silences. |
| `operate-pipelines` | Tekton inspection, dispatch, rerun, and cancellation. |
| `operate-kubernetes` | Cluster and capability discovery, resource and log reads, exact-object changes. |
| `maintain-ceph` | Native health, metrics, OSDs, devices, flags, tasks, and curated OSD maintenance. |
| `manage-machines` | Inventory, explicit host trust, and approved deploy discovery/execution. |

Each package contains its authored `SKILL.md` and one curated `references/actions.md`. The root is `skill://homelab/<name>/`, and its final path segment agrees with frontmatter `name`. `skills/list` and `skills/get` expose complete point-in-time manifests; `resources/read` serves every listed file's exact raw bytes as Markdown text. Manifest digests use SHA-256 over raw bytes and sizes measure raw bytes, not encoded transport size. Responses use private cache scope and zero TTL. The server advertises `io.modelcontextprotocol/skills` in capability extensions and the resources capability for catalog-backed reads. Directory reading and resource subscriptions are not supported; skill discovery uses `skills/list` rather than a generic resource inventory.

Assets are under `src`, which the Docker build copies and `.dockerignore` does not exclude. They compile into the binary; runtime images need no skill directory mount.

## Authority

Skill discovery and content are guidance, not operational approval. The user's initial investigation intent permits bounded reads and discovery only. Consequential changes require an explicit decision naming exact objects and requested parameters. Skills distinguish read-only tools from exec tools and distinguish listing from mutations within the mixed `machines` and `deploys` tools. They forbid automatic mutation retries and require inspection when an outcome is unknown. They omit live service URLs, credentials, public host keys, inventories, and local bootstrap commands.

The existing global required scope set remains `mcp:use kubernetes:read kubernetes:write inventory:read inventory:write inventory:host-trust deploy:read deploy:run`. The same transport authorization and Origin checks protect tools, skill discovery, and catalog resource reads. Kuri requires the entire set globally; this application does not enforce scopes per tool or action. Skill instructions do not repair or replace that limitation. Existing tool annotations, backend retry/error policies, hosted authorization, and deployed environments are unchanged.

## Coverage

[`src/mcp/tests/skills_contract.rs`](../../src/mcp/tests/skills_contract.rs) exercises actual local HTTP skill listing, exact retrieval, every manifest resource read, authored-byte equality, raw digest/size integrity, frontmatter/root agreement, unknown entries/files/cursors, exact domain action enums, removed help rejection, and missing/invalid tokens, wrong scopes, and hostile Origins. Existing MCP tests retain direct semantic filtering, synchronized text, semantic errors, images, and progress coverage. This establishes local behavior only; it does not establish a rollout, browser OAuth flow, or live infrastructure operation.
