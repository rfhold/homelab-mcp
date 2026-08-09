# Architecture

These documents define the system boundary and access model. The implementation passes local tests against a reviewed immutable Kuri Git revision.

The implementation is not committed or deployed. Preview still runs the prior health-only image, and production remains excluded.

| Document | Covers |
| --- | --- |
| [Overview](overview.md) | Components, dependencies, trust boundaries, and request flow. |
| [Access and authentication](access-authentication.md) | Hosted MCP OAuth roles, token boundaries, scopes, and durable state. |
