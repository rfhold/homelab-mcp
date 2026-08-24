---
name: pyinfra
description: Use when authoring or changing pyinfra 3.x deploys, inventories, helpers, catalog entries, or deploy integration contracts in homelab-mcp.
---

# Pyinfra

Apply pyinfra 3.x practices within the homelab-mcp deploy contracts.

This project-local skill takes precedence over same-named installed guidance for this repository.
Use it with `planning-changes` and `making-changes`; those skills own planning, edits, review, and completion.

## Required Context

1. Read the [machine deploy index](../../../docs/deploys/README.md).
2. Read [pyinfra authoring](references/pyinfra-authoring.md) for deploy or operation mechanics.
3. Read [repository contracts](references/repository-contracts.md) before any deploy-area change.
4. Inspect every affected source and coupled file before proposing a change.

## Core Rules

- Keep execution catalog-only and limited to one exact machine target.
- Require strict SSH host trust before any credential request or generation.
- Preserve short-lived OpenBao identity and all documented resource bounds.
- Preserve one active deploy, unknown post-launch outcomes, and no automatic retry.
- Prefer declarative operations and explicit operation names.
- Put environment differences in inventory, group data, or host data.
- Treat imperative and custom operations as narrow, reviewed escape hatches.

The [deploy specification](../../../docs/deploys/spec/deploys.md) remains authoritative for service behavior.

## Live-Action Gates

- Do not run `bootstrap-homelab` during routine authoring or validation.
- Do not run a live, bootstrap, SSH, OpenBao, or deploy action without exact target-specific authority.
- Require authority that names the action and exact target before any live mutation or connection.
- Do not treat `--dry` or `--debug-operations` as authority to contact a target.
- Do not commit, publish, deploy, or mutate external systems without separate exact authority.

## Completion

- The change follows the pinned Python and pyinfra versions.
- Canonical docs and coupled files agree with the implementation.
- Source-only checks pass, or the result names each unavailable check.
- Evidence distinguishes local validation from live behavior.

## References

- [Pyinfra authoring](references/pyinfra-authoring.md) - upstream pyinfra 3.x mechanics and source links.
- [Repository contracts](references/repository-contracts.md) - local sources, coupling, validation, gates, and evidence limits.
