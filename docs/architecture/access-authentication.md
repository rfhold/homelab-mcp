# Access and Authentication

## Status

This document defines the implemented hosted OAuth contract. Local tests cover selected validation, challenge, consent, and mocked OIDC behavior.

The service uses generic MCP-owned OIDC resource-owner support and PostgreSQL state. Generic migration V4 adds one-shot OIDC attempts and replaces the removed browser-state migration.

Preview runs the service at the reviewed immutable Kuri Git revision. Startup and readiness verify live PostgreSQL and signing-key access, and OIDC discovery succeeds at startup. Browser login, code/token exchange, refresh, and authenticated MCP calls remain unverified. Production remains excluded.

## Protocol Boundary

`homelab-mcp` hosts a local OAuth issuer for stateless MCP Streamable HTTP revision `2026-07-28`. The protected resource is the configured public URL whose path is exactly `/mcp`.

The resource value has exact-string semantics. Alternate origins, paths, query strings, fragments, and trailing-slash variants do not match.

Each `/mcp` request stands alone after token validation. The service requires no MCP session identifier and stores no MCP protocol session state.

Every MCP request must use a locally issued ES256 JWT access token. Each token must use JWT type `at+jwt` and contain the exact global scope set: `mcp:use kubernetes:read kubernetes:write inventory:read inventory:write inventory:host-trust deploy:read deploy:run`.

The global set gates every implemented tool, including Grafana, Tekton, Kubernetes, Ceph, machines, and deploys. Kuri requests all eight scopes automatically. It does not enforce access per tool or action.

The lack of per-tool enforcement means every authenticated MCP principal receives Ceph OSD mark, reweight, scrub, destroy, and purge authority. Preview also uses each cluster's shared Dashboard administrator credential under an explicit exception. Dedicated least-privilege accounts and explicit authorization review remain production gates.

Historical preview grants contain only `mcp:use`. Users and clients from that deployed revision must complete browser authorization again after the eight-scope set takes effect.

Authentik provides browser identity only. Authentik access tokens, ID tokens, and other Authentik credentials never authorize `/mcp`.

## Roles

| Actor | Role |
| --- | --- |
| MCP client | Discover metadata, identify or register itself, complete authorization, and call `/mcp`. |
| Generic Kuri `mcp` | Host OAuth metadata, OIDC resource-owner flow, token issuance, token validation, continuation, and durable OAuth state. |
| `homelab-mcp` | Configure Authentik and map stable issuer-plus-subject identities to local principals. |
| Authentik | Authenticate the browser user during the hosted authorization flow. |
| PostgreSQL | Persist generic OAuth and OIDC state plus protected signing material in the `mcp` schema. |

## Discovery and Bearer Challenges

The service must publish protected-resource and authorization-server metadata for the configured public resource and local issuer.

An unauthenticated `/mcp` request must return HTTP 401. Its `WWW-Authenticate` header must use the `Bearer` scheme and a `resource_metadata` parameter with the absolute protected-resource metadata URL.

An invalid, expired, or incorrectly bound token must return HTTP 401 with Bearer error `invalid_token`. A valid token that lacks any required scope returns HTTP 403 with Bearer error `insufficient_scope` and scope `mcp:use kubernetes:read kubernetes:write inventory:read inventory:write inventory:host-trust deploy:read deploy:run`.

Challenges and OAuth errors must not include tokens, authorization codes, client secrets, signing material, or OIDC transaction values.

## Client Flows

The hosted issuer supports public MCP clients through these identification paths:

- Dynamic Client Registration (DCR);
- Client ID Metadata Documents (CIMD); and
- explicit preregistration.

CIMD retrieval and DCR must preserve the hardened validation rules from the generic `mcp` crate. Redirect matching must reject open redirects and unregistered destinations.

Native clients from each supported identification path can use loopback HTTP redirects. The redirect host and path must match the registered value, and the runtime port can vary.

Authorization Code flows must require PKCE S256. The authorization request and issued code must remain bound to the client, redirect URI, exact resource, scope, and PKCE challenge.

## OIDC Resource Owner

Generic Kuri `mcp` owns the strict login and callback flow. It creates an expiring, one-shot OIDC transaction before redirecting to Authentik.

The generic flow owns state, nonce, upstream PKCE, ID-token verification, the identity-mapper seam, and hosted authorization continuation.

PostgreSQL persists only digests for state and correlation values. A secure transaction-specific cookie binds the browser to the callback.

The callback verifies state, nonce, PKCE, signature, issuer, audience, expiration, and authorization response integrity. Transaction completion is atomic and single-use.

`homelab-mcp` requests the `openid profile email` scopes from Authentik and provisions their managed property mappings. It uses only the verified issuer and subject for principal identity.

`homelab-mcp` does not own OIDC transaction persistence or callback protocol logic.

After successful authentication, hosted continuation approves only the configured `/mcp` resource and `Config::REQUIRED_OAUTH_SCOPES`. It rejects any different resource or scope. The exact set is `mcp:use kubernetes:read kubernetes:write inventory:read inventory:write inventory:host-trust deploy:read deploy:run`.

Kuri currently requires the complete set for every `/mcp` request. It does not enforce scopes per tool or action. This wiring has local test coverage but no browser or deployed verification.

Authentik session lifetime does not extend local authorization codes, access tokens, refresh generations, or OIDC transactions.

## OAuth Lifetimes

| State | Lifetime |
| --- | --- |
| Access token | 300 seconds |
| Authorization code | 300 seconds |
| Refresh generation | 86400 seconds |
| Refresh family | 2592000 seconds |
| Authorization transaction | 10 minutes |

The implementation must expire durable records and reject replay even before cleanup removes old rows.

## Token Contract

The local issuer signs access tokens with ES256. Each access token must:

- use the JWT `typ` value `at+jwt`;
- identify the configured local issuer exactly;
- bind its audience to the exact configured `/mcp` resource;
- remain within its validity interval; and
- contain every independently matched value from `Config::REQUIRED_OAUTH_SCOPES`.

The `/mcp` boundary must validate the signature, algorithm, token type, issuer, exact audience, time claims, and scope before MCP request handling.

OAuth authorization requests must use the exact resource. Token exchange and refresh must preserve that resource binding.

## Durable State and Key Protection

Generic Kuri migrations V1 through V3 own the hosted OAuth schema, signing keys, and client registration state. Migration V4 adds one-shot OIDC attempts with parent transaction cascades.

The generic dependency embeds and applies all four migrations in the `mcp` schema. No separate browser-state migration exists.

The issuer must persist ES256 signing material in encrypted form. A versioned wrapping-key file must encrypt and decrypt that material outside PostgreSQL.

The deployment must mount the wrapping key separately from database credentials. Loss of either boundary alone must not expose a usable signing key.

Database transactions must enforce expiry and atomic single-use behavior for authorization codes, refresh rotation, and OIDC transaction completion.

## Secret Boundaries

- The service must read Authentik, PostgreSQL, Grafana, and OAuth key material only from runtime secret sources.
- Tekton credentials come from separate runtime secret sources. They include the Forgejo token, PAC input secret, and projected Kubernetes token.
- Kubernetes runtime kubeconfigs use declared dedicated reduced ServiceAccount credentials for each configured cluster. They do not reuse the Tekton deployment provider kubeconfig; deployment and effective-RBAC evidence remain pending.
- Pulumi declares a different dedicated Ceph Dashboard credential boundary for each cluster through separate username and password Stashes. No seed exists yet. The credentials do not share identity with Kubernetes, Grafana, operators, or deployment providers.
- Machine deploys use a projected workload JWT and short-lived OpenBao user certificates. Inventory stores only public host pins. [Machine deploy documentation](../deploys/README.md) owns this boundary.
- The service must send the shared Grafana token only in an upstream `Authorization` header for query, render, and exec requests.
- The worktree configures the shared Grafana service account with Editor privileges because the same server-held token performs reads and creates silences; this promotion is not applied or verified live.
- Browser URLs, redirects, logs, traces, render metadata, health responses, and OAuth errors must not contain secret values. MCP image content contains only validated PNG bytes and excludes the token, origin, URL, headers, and template variables from metadata.
- Tekton MCP results, telemetry, and errors exclude MCP-held secret values. Task logs retain the documented residual risk for arbitrary workload secrets.
- The service must not persist plaintext OAuth signing keys.
- Build output and container layers must not contain private Git or provider credentials.
- Production requires separate target-specific approval and validated rotation and recovery procedures.
