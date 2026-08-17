# SSH Trust And OpenBao Identity

## Separate Trust Directions

SSH needs independent client and server authentication:

| Direction | Mechanism |
| --- | --- |
| Runtime to machine identity | OpenBao signs a fresh Ed25519 client public key as a user certificate. |
| Machine to runtime identity | Inventory supplies one exact Ed25519 host public key for strict host checking. |

The implementation does not use OpenBao host certificates. It does not use `ssh-keyscan` or trust on first use.

## Workload Authentication

The live platform Kubernetes auth backend and Tekton CI identity are separate from this application workload identity. Platform ownership and positive/negative canaries establish `pipelines-as-code/openbao-pulumi-admin-v1` for Pulumi administration. The shared `openbao-kubernetes-login` StepAction is live and converged; this repository's use of its memory-only same-pod token file and apply-step `VAULT_TOKEN` export remains source-declared and has not run.

Pulumi preview declarations project a 600-second ServiceAccount token. Its audience comes from `homelab-mcp:openbaoAudience`. The application reads this JWT from `HOMELAB_MCP_OPENBAO_JWT_PATH`.

`OpenBaoClient::request_certificate` authenticates at the configured Kubernetes auth mount and role. The returned service token has only update access to the configured SSH sign route.

For each run, `DeployRunner` creates a private directory in `HOMELAB_MCP_DEPLOY_TEMP_ROOT`. It generates a fresh Ed25519 keypair and requests a user certificate with:

- certificate type `user`;
- principal `homelab`; and
- TTL `15m`.

The preview Pulumi role also limits user certificates to `homelab`, disables host certificates, and caps TTL at 15 minutes.

The runner writes private material with restrictive modes on a memory-backed 16 MiB volume. It clears the child environment, disables stdin, uses fixed absolute executables, and deletes the run directory after completion.

## Host Authentication And Bounds

The runner writes one exact `known_hosts` entry from the inventory pin. pyinfra receives `ssh_strict_host_key_checking: yes` and the generated known-hosts path.

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

Preview declarations enable the application-owned SSH mount, CA, role, sign policy, and runtime Kubernetes role. They remain unapplied and unverified. Production configuration sets `openbaoEnabled: false` and `openbaoCreateSshMount: false`; production also remains unapplied.

The declared `openbaoEndpointCidrs` need independent verification before any apply. Vault provider `7.11.0` is the exact platform-tested version, but Pulumi mocks prove only this repository's declaration shape. They do not prove network reachability, the first apply, application-owned OpenBao resources, certificate issuance, or SSH login.
