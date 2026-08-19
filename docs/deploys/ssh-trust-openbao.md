# SSH Trust And OpenBao Identity

## Separate Trust Directions

SSH needs independent client and server authentication:

| Direction | Mechanism |
| --- | --- |
| Runtime to machine identity | OpenBao signs a fresh Ed25519 client public key as a user certificate. |
| Machine to runtime identity | Inventory supplies one exact Ed25519 host public key for strict host checking. |
| Human to machine identity | The local `ssh_homelab` helper obtains a 15-minute OpenBao user certificate after human OIDC login. |

The implementation does not use OpenBao host certificates. It does not use `ssh-keyscan` or trust on first use.

## Human Authentication

`deploys.helpers.ssh_homelab` generates a fresh Ed25519 key in a private temporary directory for each session. It validates the configured OpenBao URL as an HTTPS root origin, sets `BAO_ADDR` explicitly, and forces `BAO_DISABLE_REDIRECTS=true` for login, lookup, revocation, and signing. Bao receives a minimal environment that omits ambient proxy, custom CA, skip-verification, agent, namespace, token, and Vault compatibility settings. OIDC login uses `-token-only`, so Bao's normal token helper is not read or overwritten. The captured token is never printed or placed in command arguments.

Only this OIDC `ssh` token is cached. `$XDG_RUNTIME_DIR` must be absolute, non-symlinked, current-user-owned, and inaccessible to group and other users. The cache uses a mode `0700` application directory, a mode `0600` regular file, bounded JSON, canonical-origin association, and atomic replacement. It rejects unsafe filesystem objects and never persists under `$HOME`. Bao lookup must prove service token type, role metadata `ssh`, exact `homelab-ssh-client-sign` policy without broad/default/identity policies, and positive remaining TTL no greater than eight hours before reuse or save. Authentication-rejected or expired cached tokens are cleared directly. Successfully looked-up tokens that violate the expected contract are best-effort revoked before clearing and replacement. Indeterminate transport or response failures preserve an existing cache and fail closed. Invalid or indeterminate new tokens are best-effort revoked and are not cached. A signing-time authentication rejection also triggers best-effort revocation before the cache is cleared and one login/sign retry.

The eight-hour bound is a maximum accepted cache lifetime, not evidence that the live platform role currently issues eight-hour tokens. Platform-side role and policy changes are owned and deployed separately. A stolen cached bearer token can request principal-`homelab` certificates until revocation or token expiry; each certificate can remain useful for up to 15 minutes. Volatile storage, ownership, and modes reduce persistence and cross-user exposure but do not protect against same-user process compromise or root. Use `--reauth` to revoke and replace the cache, and `--logout` to best-effort self-revoke and clear it without a target.

The request fixes `cert_type=user`, `valid_principals=homelab`, and `ttl=15m`. The helper independently requires a current user certificate, sole principal `homelab`, no more than 15 minutes remaining, no more than a 15-minute 30-second total interval, and no future start. The 30-second allowance reflects the exact managed `not_before_duration`; it does not extend useful life beyond 15 minutes. OpenSSH receives `-F none`, the temporary identity, certificate, `IdentitiesOnly=yes`, `BatchMode=yes`, `PreferredAuthentications=publickey`, and strict known-hosts options. Cleanup runs on normal return and failure. The helper does not use a default private key, prompt for another authentication method after issuance, or add a persistent `authorized_keys` entry.

## Workload Authentication

The live platform Kubernetes auth backend and Tekton CI identity are separate from this application workload identity. Platform ownership and positive/negative canaries establish `pipelines-as-code/openbao-pulumi-admin-v1` for Pulumi administration. The shared `openbao-kubernetes-login` StepAction is live and converged; this repository's use of its memory-only same-pod token file and apply-step `VAULT_TOKEN` export remains source-declared and has not run.

Pulumi preview declarations project a 600-second ServiceAccount token. Its audience comes from `homelab-mcp:openbaoAudience`. The application reads this JWT from `HOMELAB_MCP_OPENBAO_JWT_PATH`.

`OpenBaoClient::request_certificate` authenticates at the configured Kubernetes auth mount and role. The returned service token has only update access to the configured SSH sign route.

For each run, `DeployRunner` creates a private directory in `HOMELAB_MCP_DEPLOY_TEMP_ROOT`. It generates a fresh Ed25519 keypair and requests a user certificate with:

- certificate type `user`;
- principal `homelab`; and
- TTL `15m`.

The preview Pulumi role also limits user certificates to `homelab`, disables host certificates, caps TTL at 15 minutes, and now declares `notBeforeDuration: "30s"` explicitly.

The runner writes private material with restrictive modes on a memory-backed 16 MiB volume. After writing the signed certificate, it removes the generated plain public key before starting pyinfra, leaving only the private key and `identity-cert.pub` for credential loading. It clears the child environment, disables stdin, uses fixed absolute executables, and deletes the run directory after completion.

## Host Authentication And Bounds

The runner writes one exact `known_hosts` entry from the inventory pin. pyinfra receives `ssh_strict_host_key_checking: yes` and the generated known-hosts path.

The operator bootstrap and human SSH helpers instead require a pre-established local known-hosts file, defaulting to `$HOME/.ssh/known_hosts`. Bootstrap is agent-first and uses the inherited SSH agent through its custom bounded Paramiko strategy, with password fallback only when the server advertises it. pyinfra's native agent handling, local key-file discovery, and user SSH configuration remain disabled, and its SSH config file is `/dev/null`. Human SSH uses `-F none`. User `IdentityFile`, `HostName`, `ProxyCommand`, `ProxyJump`, and `Match exec` settings cannot affect either path. Neither helper weakens host checking.

The runner limits key generation to 10 seconds. Each catalog entry limits deploy duration to at most 900 seconds. Current `system-info` uses 30 seconds.

Stdout has a 512 KiB limit. Stderr has a 32 KiB limit. Cancellation, timeout, and overflow kill the process group and report an unknown execution outcome where applicable.

OpenBao requests require an HTTPS root origin, reject redirects and proxies, and use bounded request and response sizes. Error results omit JWTs, service tokens, public keys, certificates, command output, and paths.

## Configuration Authority

`Config::from_env` applies prefix `HOMELAB_MCP_`. `DeployIntegrationConfig` defines these required suffixes:

- `DEPLOY_ROOT`
- `DEPLOY_CATALOG`
- `DEPLOY_UV_EXECUTABLE`
- `DEPLOY_SSH_KEYGEN_EXECUTABLE`
- `DEPLOY_TEMP_ROOT`
- `OPENBAO_URL`
- `OPENBAO_KUBERNETES_AUTH_MOUNT`
- `OPENBAO_KUBERNETES_ROLE`
- `OPENBAO_SSH_MOUNT`
- `OPENBAO_SSH_ROLE`
- `OPENBAO_JWT_PATH`
- `OPENBAO_REQUEST_TIMEOUT_MS`

`infra/pulumi/index.ts` supplies these values from `deployRoot`, `deployTempRoot`, and the `openbao*` stack configuration keys. `infra/pulumi/policy.ts` validates safe path segments, CIDRs, ports, and preview-only enablement.

Preview declarations enable the application-owned SSH mount, CA, role, sign policy, and runtime Kubernetes role. They remain unapplied and unverified, including the explicit 30-second not-before declaration. Production configuration sets `openbaoEnabled: false` and `openbaoCreateSshMount: false`; production also remains unapplied.

The declared `openbaoEndpointCidrs` need independent verification before any apply. Vault provider `7.11.0` is the exact platform-tested version, but Pulumi mocks prove only this repository's declaration shape. They do not prove network reachability, the first apply, application-owned OpenBao resources, certificate issuance, or SSH login.
