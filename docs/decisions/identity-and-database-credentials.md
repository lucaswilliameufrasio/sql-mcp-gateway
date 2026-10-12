# Identity and database credential model

Status: accepted product direction; local Vault/PostgreSQL/MySQL dynamic-credential and privilege behavior is validated by `make test-integration`. Deployment-specific identity, network, and TLS policies still require environment-level verification.

## Context

The gateway must support multiple people accessing PostgreSQL and MySQL through MCP, with an administrator granting read/write access per user and per registered database. A shared database account would make authorization and attribution too coarse.

## Decision

- Authenticate people through an OIDC-provider-agnostic interface.
- Use Vault as the first database credential broker.
- Issue a short-lived, unique database identity per session and access mode. Correlate the Vault lease/database principal to the OIDC issuer and subject in gateway audit events.
- Store user/database grants in the gateway control plane. Deny access without an explicit grant.
- Enforce the grant both in the gateway and in the privileges attached to the Vault-issued database identity. The effective permission is the intersection of the two.
- Keep SQL Manager as a separate desktop product; build the gateway as a separate service.
- Implement the gateway in Rust and the administration web application in SvelteKit.

## Consequences

- PostgreSQL and MySQL require separate Vault/database role configuration and adapter validation; OIDC compatibility does not make database credential issuance identical across engines.
- Database logs will show a temporary database principal rather than necessarily the person's permanent username. Gateway audit events must preserve the identity correlation.
- Database root credentials belong in Vault, not the gateway's metadata store. Admin forms must be write-only for secret fields.
- The stdio transport must authenticate and relay to the same gateway policy path as remote MCP; it must not make direct database connections with broader credentials.
- Lease expiry is required as a cleanup fallback; session teardown should revoke leases and close connections when possible.

## Revisit if

The requirement changes to mandate permanent human usernames in native database logs, or if the chosen database/Vault integration cannot provide distinct, least-privilege credentials for concurrent sessions.
