# Access and Authentication

## Status

This document defines the intended authentication and authorization contract. No OAuth endpoint, browser handler, token issuer, or database use exists yet.

Pulumi declares an Authentik browser application, signing certificate, application credentials, PostgreSQL, and a versioned wrapping-key secret. These declarations prepare runtime inputs but do not implement the access flow.

## Boundary

`homelab-mcp` will host its own OAuth issuer. Authentik will authenticate the browser user through a confidential browser application.

Only locally issued ES256 JWT access tokens with the `at+jwt` type will reach `/mcp`. The service will reject direct Authentik tokens.

Every accepted access token must include the `mcp:use` scope.

## Planned Roles

| Actor | Role |
| --- | --- |
| MCP client | Discover metadata, register or identify itself, complete authorization, and call `/mcp`. |
| homelab-mcp | Host OAuth metadata, authorization flows, token issuance, token validation, and MCP authorization. |
| Authentik | Authenticate the browser user for the hosted authorization flow. |
| PostgreSQL | Persist generic OAuth state and browser-auth state. |

## Client Flows

The hosted issuer will enable Dynamic Client Registration (DCR), Client ID Metadata Documents (CIMD), and loopback redirects. Implementations must preserve redirect validation and OAuth binding checks from the pinned generic `mcp` crate.

## OAuth Lifetimes

| State | Lifetime |
| --- | --- |
| Access token | 300 seconds |
| Authorization code | 300 seconds |
| Refresh generation | 86400 seconds |
| Refresh family | 2592000 seconds |
| Authorization transaction | 10 minutes |

Browser-session lifetime and cleanup cadence remain implementation decisions.

## Token Contract

The local issuer will sign access tokens with ES256. Each access token must:

- use the JWT `typ` value `at+jwt`;
- identify the local issuer;
- target the configured MCP resource;
- remain within its validity interval; and
- contain `mcp:use`.

The `/mcp` boundary must validate the signature, token type, issuer, audience or resource binding, time claims, and scope.

## Durable Key Protection

PostgreSQL will store generic OAuth state, including protected signing-key material. A versioned wrapping-key file will protect persisted signing keys.

The deployment must mount the wrapping key separately from database credentials. Key rotation and recovery procedures require implementation and operational validation before production readiness.

## Browser Session State

PostgreSQL will store browser-auth state that binds the hosted OAuth transaction to the Authentik callback. The implementation must apply expiration, single-use completion, and anti-forgery checks.

## Security Invariants

- Authentik credentials and tokens must not authorize `/mcp` directly.
- Grafana credentials must not appear in browser redirects, MCP results, or logs.
- OAuth signing keys must not persist as unprotected database values.
- Redirect handling must allow configured DCR, CIMD, and loopback use without open redirects.
- Authentication failures must not disclose token, key, or browser-session secrets.
