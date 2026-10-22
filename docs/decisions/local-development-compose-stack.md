# Local development Compose stack

Status: accepted for local development only. This stack is not a production deployment profile.

## Context

The gateway requires a control-plane PostgreSQL database, an OIDC issuer, Vault database secrets engines, and reachable PostgreSQL/MySQL targets. Developers need a reproducible local environment without changing the provider-agnostic production OIDC contract or weakening the gateway's transport checks.

## Decision

- Provide a separate `compose.dev.yaml` for Authentik, Vault dev mode, a dedicated control-plane PostgreSQL database, TLS-enabled PostgreSQL/MySQL sample targets, and an opt-in app profile containing the gateway and SvelteKit admin UI.
- Pin the local Authentik image and configure its public Authorization Code OIDC client and `dev-admin` test user through a versioned Authentik blueprint mounted read-only into the worker. Use an asymmetric signing key because the gateway validates access-token signatures through the provider's JWKS endpoint. The browser client sends S256 PKCE.
- Keep HTTP OIDC and Vault URLs on `127.0.0.1`, as required by the gateway's existing local-only HTTP validation. Authentik, Vault, gateway, and admin UI share a Compose network namespace so those loopback URLs and the existing UI API proxy work inside their containers while the gateway can still resolve database services on a separate Compose bridge network. Publish only Authentik, Vault, gateway, and admin UI ports on host loopback; do not publish database ports. The bridge uses Docker's normal outbound networking.
- Generate local credentials, including separate Authentik bootstrap and `dev-admin` passwords, in owner-only `.env.dev`; store Vault's scoped runtime token in `.dev/vault-runtime/runtime.env`, and database TLS keys/certificates under ignored `.dev/tls/`. Never reuse these values or local certificates outside this development stack.
- Use Vault dev mode with a distinct runtime token restricted to database credential issuance and lease revocation. Vault dev storage is in-memory; pause/resume the stack to preserve it during a work session.

## Consequences

- The local Authentik provider, application, and test user are reproducible from Compose, while production gateway identity configuration remains provider-agnostic. The Authentik worker health check waits for the configured issuer's discovery endpoint, so the gateway starts only after the blueprint has been applied.
- The SvelteKit admin UI shares Authentik's local network namespace so its existing loopback API proxy reaches the gateway. This is a local-only arrangement and is not a production deployment profile.
- The gateway and Authentik/Vault share a network namespace. This is an intentional local-only trade-off to preserve strict loopback HTTP validation; it must not be copied to production.
- Containers have normal bridge-network egress, although the database services have no host-published ports and the user-facing service ports bind only to host loopback.
- The generated target certificates are self-signed and only suitable for local testing. The gateway's database registration flow must receive the local CA certificate.
- Recreating Vault dev mode loses its database configuration and leases while the control/target database volumes may persist. Re-register targets after Vault state is lost; do not use volume-removal commands unless intentionally resetting local data.
- The Vault root token is used only for the local gateway administration client. It grants broad local privileges and is not an example for production credentials.
