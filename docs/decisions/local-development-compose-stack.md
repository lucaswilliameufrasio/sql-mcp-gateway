# Local development Compose stack

Status: accepted for local development only. This stack is not a production deployment profile.

## Context

The gateway requires a control-plane PostgreSQL database, an OIDC issuer, Vault database secrets engines, and reachable PostgreSQL/MySQL targets. Developers need a reproducible local environment without changing the provider-agnostic production OIDC contract or weakening the gateway's transport checks.

## Decision

- Provide a separate `compose.dev.yaml` for Authentik, Vault dev mode, a dedicated control-plane PostgreSQL database, and TLS-enabled PostgreSQL/MySQL sample targets. The gateway is opt-in under the Compose `app` profile; the SvelteKit admin UI continues to run with `make admin-dev` on the host.
- Pin the local Authentik image and use a manually configured public Authorization Code + PKCE client. The client must use an asymmetric signing key because the gateway validates access-token signatures through the provider's JWKS endpoint.
- Keep HTTP OIDC and Vault URLs on `127.0.0.1`, as required by the gateway's existing local-only HTTP validation. Authentik, Vault, and the gateway share a Compose network namespace so those loopback URLs work inside the gateway while it can still resolve database services on a separate Compose bridge network. Publish only Authentik, Vault, and gateway ports on host loopback; do not publish database ports. The bridge uses Docker's normal outbound networking.
- Generate local credentials in owner-only `.env.dev` and `.dev/vault-runtime/runtime.env`, and database TLS keys/certificates under ignored `.dev/tls/`. Never reuse these values or local certificates outside this development stack.
- Use Vault dev mode with a distinct runtime token restricted to database credential issuance and lease revocation. Vault dev storage is in-memory; pause/resume the stack to preserve it during a work session.

## Consequences

- Initial Authentik application/client setup remains a manual step so the repo does not hard-code an IdP-specific production configuration. Developers must keep the configured issuer, client ID/audience, redirect URI, and bootstrap admin subject in sync with `.env.dev`.
- The gateway and Authentik/Vault share a network namespace. This is an intentional local-only trade-off to preserve strict loopback HTTP validation; it must not be copied to production.
- Containers have normal bridge-network egress, although the database services have no host-published ports and the user-facing service ports bind only to host loopback.
- The generated target certificates are self-signed and only suitable for local testing. The gateway's database registration flow must receive the local CA certificate.
- Recreating Vault dev mode loses its database configuration and leases while the control/target database volumes may persist. Re-register targets after Vault state is lost; do not use volume-removal commands unless intentionally resetting local data.
- The Vault root token is used only for the local gateway administration client. It grants broad local privileges and is not an example for production credentials.
