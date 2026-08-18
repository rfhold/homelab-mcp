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

Bootstrap requires current administrator SSH access through the inherited SSH agent or, when the server advertises it, password authentication. It also requires an independently verified host entry in `known_hosts`. Host trust remains strict; the helper does not use trust on first use or `ssh-keyscan`. The default trust file is `$HOME/.ssh/known_hosts`.

Warning: `bootstrap-homelab` mutates sudo and sshd configuration. It creates a passwordless administrator with `NOPASSWD: ALL`. Review the target and maintain a separate access path.

From the repository root, run the local helper with the exact target and current administrator user:

```bash
uv run --locked python -m deploys.helpers.bootstrap_homelab \
  <machine-host> <current-admin-user>
```

Optional flags are `--port`, `--known-hosts`, `--ca-url`, and `--separate-sudo-password`. The CA URL defaults to `https://openbao.holdenitdown.net/v1/homelab-ssh-client/public_key`. Overrides must use HTTPS.

The helper downloads one bounded Ed25519 CA public key without authentication. It rejects redirects, proxies, malformed responses, multiple keys, non-Ed25519 keys, and oversized responses. It uses the Python runtime's compiled system CA paths and ignores ambient `SSL_CERT_FILE` and `SSL_CERT_DIR` overrides. It writes the CA key to a mode `0600` file in a private temporary directory, constructs non-secret inventory JSON, invokes the fixed locked pyinfra bootstrap, and removes the temporary directory on exit.

The custom Paramiko strategy probes the server's advertised methods with none-auth. When `publickey` is available, it opens `paramiko.Agent()` through the inherited `SSH_AUTH_SOCK` and tries only the first four agent identities in their original order. Additional identities are not attempted or enumerated by the strategy. The four-attempt budget avoids consuming typical server `MaxAuthTries` limits and preserves room for advertised password fallback. Authentication uses `InMemoryPrivateKey` with each `AgentKey`; private key material is never read or persisted. Agent signing leaves GPG pinentry and hardware touch handling with the inherited agent. The helper does not invoke `gpg`, `ssh-add`, or key files. The agent connection closes after success and on every failure path.

If no bounded agent identity succeeds and the server advertises `password`, the strategy lazily prompts once through Python `getpass` and performs strict password authentication with keyboard-interactive fallback disabled. A publickey-only server never triggers an SSH password prompt and instead returns a credential-safe actionable failure. Agent key representations expose only bounded algorithm and fingerprint metadata; passwords, key blobs, and private material are absent from arguments, environment variables, files, errors, logs, and representations.

pyinfra's native agent and local key discovery remain disabled because the custom strategy exclusively owns agent access. pyinfra uses `/dev/null` as its SSH config file, so ProxyCommand, ProxyJump, and `Match exec` cannot affect bootstrap. The exact target, user, port, strict known-hosts file, and `ssh_strict_host_key_checking: yes` remain fixed.

Sudo authentication is separate and lazy. After agent success, the sudo password is requested only when the privileged operation consumes it. In the default mode, an SSH fallback password is cached in redacted memory and reused for sudo; otherwise sudo receives its own lazy prompt. `--separate-sudo-password` always uses an independent lazy sudo prompt. Sudo invalidates its timestamp and reads the secret only from SSH channel stdin; the deploy does not use pyinfra's environment-based sudo password path. The privileged child closes stdin before package or configuration work, including the NOPASSWD case.

The command uses operator-local administrator access. CI and MCP never receive the password. `bootstrap-homelab` remains local-only and unavailable through MCP.

The deploy installs OpenSSH and sudo packages where required. It creates `homelab` with `/bin/sh` and a home directory. If the `homelab` group already exists, a new `homelab` user joins that group. Existing users retain their current primary group; the deploy obtains that group with `id -gn homelab` and uses it when reconciling `/home/homelab` ownership rather than forcing a group named `homelab`. The deploy validates the fixed sudoers file with `visudo` before installation.

The deploy installs the supplied user CA key at `/etc/ssh/trusted-user-ca-keys.pem`. It adds `TrustedUserCAKeys` through `/etc/ssh/sshd_config.d/90-homelab-user-ca.conf`. It validates a candidate configuration and the final sshd configuration before reload.

The procedure preserves current user accounts, authorized keys, and base sshd configuration. It does not remove the operator's current access. OpenBao host certificates are not implemented.

## Human SSH Session

After bootstrap, open a human session with:

```bash
uv run --locked python -m deploys.helpers.ssh_homelab <machine-host>
```

Optional flags are `--port`, `--known-hosts`, `--openbao-url`, and `--reauth`. The OpenBao origin defaults to `https://openbao.holdenitdown.net`. An override must be a canonical HTTPS root origin without credentials, a query, or a fragment. The helper requires an existing known-hosts file and always sets `StrictHostKeyChecking=yes`. `--reauth` best-effort revokes and clears a matching cached token before opening the browser login.

The helper requires an absolute `$XDG_RUNTIME_DIR` that is a non-symlink directory owned by the current user with no group or other access. It creates `$XDG_RUNTIME_DIR/homelab-mcp` with mode `0700` and stores `ssh-token.json` with mode `0600` using bounded JSON and atomic replacement. Unsafe roots, cache directories, symlinks, non-regular files, wrong ownership, and permissive modes are rejected. Malformed, oversized, or origin-mismatched cache content is removed rather than used. The helper never falls back to `$HOME`.

Before reuse, Bao looks up the cached token with the token only in a minimal child environment. Every Bao child receives `BAO_DISABLE_REDIRECTS=true`; hostile inherited values cannot enable redirects. The helper accepts only a service token for OIDC role metadata `ssh`, exact policy `homelab-ssh-client-sign`, no identity/default/admin policy, and positive remaining TTL no greater than eight hours. A lookup that confirms authentication rejection or expiration clears the unusable cache directly. A successful live lookup that violates the policy, role, type, TTL, identity-policy, or issuance-path contract is best-effort revoked before being cleared and replaced; unconfirmed revocation produces a credential-safe warning. Transport, timeout, protocol, malformed JSON, and oversized lookup failures are indeterminate: the helper preserves an existing cache and fails closed with retry/`--reauth` guidance. A newly issued token is cached only after confirmed validation; invalid or indeterminate new tokens are best-effort revoked and never cached. The Bao token helper and broad administrator credentials are neither read nor overwritten.

If signing classifies a validated cached token as rejected, the helper first best-effort self-revokes it, emits a credential-safe warning if revocation cannot be confirmed, clears the cache, and performs one login/sign retry. Arbitrary transport, server, malformed, oversized, and SSH outcomes are not retried.

Each SSH invocation still creates a private temporary directory and fresh Ed25519 key. Signing requests `homelab-ssh-client/sign/homelab` with principal `homelab` and TTL `15m`. Before connecting, the helper checks the returned certificate with `LC_ALL=C`, `TZ=UTC`, and `ssh-keygen -L`. It requires a current user certificate whose sole principal is `homelab`, whose remaining life is no more than 15 minutes, whose total interval is no more than 15 minutes 30 seconds, and whose validity never begins in the future. The additional 30 seconds is the exact managed signer backdate, not additional useful certificate life. OpenSSH receives `-F none`, the explicit temporary key and certificate, `IdentitiesOnly=yes`, `BatchMode=yes`, `PreferredAuthentications=publickey`, and the strict known-hosts file. This makes the post-issuance connection certificate-only and noninteractive. The helper removes the key, public key, certificate, and temporary directory when SSH exits or an earlier step fails.

End the cached browser session without a target:

```bash
uv run --locked python -m deploys.helpers.ssh_homelab --logout
```

Logout succeeds when no cache exists. When a token exists, it best-effort runs self-revocation and always clears the local cache; a safe warning explains when the token may remain valid until server-side expiry.

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
