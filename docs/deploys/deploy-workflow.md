# Deploy Workflow

## Project And Catalog

`pyproject.toml` defines a Python 3.13 uv project with exact dependency `pyinfra==3.5.0`. `uv.lock` locks the environment. `deploys/catalog.json` is the strict workflow catalog.

The catalog rejects unknown fields, duplicate or unsafe IDs, oversized input, invalid timeouts, and paths outside the canonical deploy root. It supports Debian, Ubuntu, and Arch Linux.

| Deploy | Availability | Behavior |
| --- | --- | --- |
| `bootstrap-homelab` | Operator-local only | Installs SSH and sudo prerequisites, creates `homelab`, grants fixed sudo authority, and installs SSH user CA trust. |
| `system-info` | Local and MCP | Reads bounded operating system, capacity, storage, and network facts. |

`deploys/fixtures/compose.yaml` declares isolated Debian and Arch containers for fixture work. Repository checks validate the Compose configuration. They do not pull images or execute containers.

## Operator Bootstrap

Bootstrap requires current administrator SSH access, one independently verified host key file, and the OpenBao SSH user CA public key. Use absolute paths in the inventory JSON.

Warning: `bootstrap-homelab` mutates sudo and sshd configuration. It creates a passwordless administrator with `NOPASSWD: ALL`. Review the target and maintain a separate access path.

From the repository root, use this command shape with placeholders:

```bash
HOMELAB_INVENTORY_JSON='{"version":1,"host":{"address":"<machine-host>","user":"<current-admin-user>","port":22,"ssh_key":"/absolute/path/to/current-admin-key","known_hosts":"/absolute/path/to/verified-known-hosts"},"ca_public_key_file":"/absolute/path/to/openbao-user-ca.pub"}' \
  uv run --locked pyinfra --yes \
  deploys/inventory_bootstrap.py \
  deploys/entrypoints/bootstrap_homelab.py
```

The command uses operator-local administrator SSH. CI and MCP never receive that administrator key.

The deploy installs OpenSSH and sudo packages where required. It creates `homelab` with `/bin/sh` and a home directory. It validates the fixed sudoers file with `visudo` before installation.

The deploy installs the supplied user CA key at `/etc/ssh/trusted-user-ca-keys.pem`. It adds `TrustedUserCAKeys` through `/etc/ssh/sshd_config.d/90-homelab-user-ca.conf`. It validates a candidate configuration and the final sshd configuration before reload.

The procedure preserves current user accounts, authorized keys, and base sshd configuration. It does not remove the operator's current access. OpenBao host certificates are not implemented.

## MCP Run

The progressive `deploys` tool exposes `list` and `run`. A run accepts only `deploy_id` and one exact `machine_id` UUID. The catalog supplies every entrypoint, inventory, timeout, and availability decision.

MCP currently resolves only `system-info`. Callers cannot supply shell commands, pyinfra arguments, inventory fields, SSH options, paths, environment variables, or additional targets.

The runtime executes this fixed command shape in the canonical deploy root:

```text
uv run --locked pyinfra --yes <catalog-inventory> <catalog-entrypoint>
```

Only one deploy can run at a time. Cancellation, timeout, process failure, or excess output can produce an unknown outcome. Inspect the target through a separately authorized read before retry.

## Local Checks

These commands validate the locked Python project and deploy contracts without contacting a machine:

```bash
uv lock --check
PYTHONDONTWRITEBYTECODE=1 uv run --locked python -m unittest discover -s deploys/tests
docker compose -f deploys/fixtures/compose.yaml config --quiet
```

The Docker command validates configuration only. Do not claim container execution unless an authorized check pulls and starts the images.
