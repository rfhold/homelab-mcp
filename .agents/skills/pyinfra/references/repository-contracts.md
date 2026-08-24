# Homelab-MCP Deploy Contracts

Use this map before changes under `deploys/` or `src/integrations/deploys/`. Canonical service behavior remains in [`docs/deploys/`](../../../../docs/deploys/README.md).

## Authority Map

| Source | Authority |
| --- | --- |
| [`docs/deploys/spec/deploys.md`](../../../../docs/deploys/spec/deploys.md) | Normative catalog, target, trust, identity, process, bounds, and outcome requirements. |
| [`docs/deploys/spec/machines.md`](../../../../docs/deploys/spec/machines.md) | Normative machine identity, host-pin, persistence, and mutation requirements. |
| [`docs/deploys/deploy-workflow.md`](../../../../docs/deploys/deploy-workflow.md) | Operator procedures, fixed invocation, local checks, and recovery guidance. |
| [`docs/deploys/ssh-trust-openbao.md`](../../../../docs/deploys/ssh-trust-openbao.md) | SSH trust directions, credential lifecycle, bounds, and declaration status. |
| [`deploys/catalog.json`](../../../../deploys/catalog.json) | Approved deploy metadata, paths, availability, risk, and timeout values. |
| [`pyproject.toml`](../../../../pyproject.toml) and [`uv.lock`](../../../../uv.lock) | Python and dependency versions for the locked deploy environment. |

## Implementation Map

| Surface | Inspect |
| --- | --- |
| Deploy entrypoints and reusable logic | [`deploys/entrypoints/`](../../../../deploys/entrypoints/), [`deploys/lib/`](../../../../deploys/lib/), and [`deploys/helpers/`](../../../../deploys/helpers/) |
| Inventory data and SSH connector controls | [`deploys/inventory_system_info.py`](../../../../deploys/inventory_system_info.py), [`deploys/inventory_bootstrap.py`](../../../../deploys/inventory_bootstrap.py), and [`deploys/lib/inventory.py`](../../../../deploys/lib/inventory.py) |
| Catalog parsing and path constraints | [`src/integrations/deploys/catalog.rs`](../../../../src/integrations/deploys/catalog.rs) |
| Runtime process, concurrency, output, and cleanup | [`src/integrations/deploys/runner.rs`](../../../../src/integrations/deploys/runner.rs) |
| OpenBao certificate request | [`src/integrations/deploys/openbao.rs`](../../../../src/integrations/deploys/openbao.rs) |
| Result protocol normalization | [`src/integrations/deploys/normalize.rs`](../../../../src/integrations/deploys/normalize.rs) |
| Python contract coverage | [`deploys/tests/test_contracts.py`](../../../../deploys/tests/test_contracts.py) and [`deploys/tests/test_helpers.py`](../../../../deploys/tests/test_helpers.py) |

## Coupled Files

| When this changes | Also inspect |
| --- | --- |
| A catalog entry or path | Catalog tests, Rust catalog parsing, deploy specification, and deploy workflow. |
| An entrypoint or reusable deploy | Its inventory, catalog entry, Python contract tests, and output normalization. |
| Inventory fields or SSH options | Both inventory modules, Rust inventory serialization, trust docs, and contract tests. |
| Operation output or callbacks | Rust capture limits, protocol normalization, failure classification, and Python tests. |
| Timeout, output, or concurrency bounds | Catalog, runner, deploy specification, workflow, and Rust tests. |
| OpenBao or SSH credential behavior | Runner, OpenBao client, inventory, trust documentation, Pulumi declarations, and security tests. |

## Security Boundaries

| Boundary | Canonical source |
| --- | --- |
| Catalog selection, exact target, fixed process, resource bounds, and outcome handling | [Deploy execution requirements](../../../../docs/deploys/spec/deploys.md#requirements) |
| Host-pin mutation and inventory secrets | [Machine inventory requirements](../../../../docs/deploys/spec/machines.md#requirements) |
| Bootstrap access and operator recovery | [Operator bootstrap](../../../../docs/deploys/deploy-workflow.md#operator-bootstrap) |
| SSH trust and credential lifecycle | [SSH trust and OpenBao identity](../../../../docs/deploys/ssh-trust-openbao.md) |

## Routine Validation

Run these source-only checks from the repository root:

```bash
uv lock --check
PYTHONDONTWRITEBYTECODE=1 uv run --locked python -m unittest discover -s deploys/tests
docker compose -f deploys/fixtures/compose.yaml config --quiet
```

Routine validation must not run `bootstrap-homelab`.

The Docker command validates Compose configuration only. It does not pull images or start containers.

## Evidence Limits

These checks prove lock consistency, local Python contracts, and Compose syntax. They do not prove SSH reachability, host trust, OpenBao issuance, live machine behavior, container execution, preview rollout, or production state.

Report each live layer as unverified unless target-specific authorized evidence proves it. Preserve the current evidence boundaries in the [deploy index](../../../../docs/deploys/README.md) and [SSH trust documentation](../../../../docs/deploys/ssh-trust-openbao.md).
