# ADR 0001: First-class MCP client onboarding

- Status: Accepted
- Date: 2026-10-24

## Context

The gateway implements MCP tools over HTTP and stdio, but connecting a client
currently requires manually supplying an OIDC access token. The HTTP endpoint
does not advertise MCP OAuth metadata, and the stdio process cannot start an
interactive login. This makes otherwise compatible clients difficult to set
up and encourages unsafe token handling in client configuration.

## Decision

Make remote HTTP with browser-based OAuth the primary client journey, and keep
stdio as a first-class alternative. The gateway will provide MCP-compatible
OAuth authorization-server discovery and client onboarding while delegating
human authentication to the configured OIDC issuer. Tokens used against the
gateway must remain audience-bound to this gateway, and database access
continues to use the existing per-principal/per-database grants and Vault
leases. Client setup must never require putting a bearer token in a checked-in
or shared configuration file.

Provide client-specific, copyable setup for OpenCode and Claude Code plus a
generic MCP configuration path. Keep the protocol and authorization behavior
shared across clients; client-specific material is configuration/documentation,
not separate authorization logic.

## Alternatives considered

- Keep manual OIDC bearer tokens and add only client snippets: rejected because
  tokens expire and are unsafe to distribute in client configuration.
- Require each MCP client to be registered directly in every OIDC provider:
  rejected as the only path because provider support for MCP client registration
  varies and makes onboarding provider-specific.
- Support only stdio with an environment-injected token: rejected because it
  does not provide a convenient remote endpoint and still leaves token renewal
  to the user.

## Consequences

- This is an authentication feature, not merely a documentation change. It must
  implement OAuth authorization-code protections (including PKCE, exact redirect
  URI validation, state/CSRF protection, one-time codes, token audience binding,
  and safe token storage) and MCP protected-resource/authorization-server
  discovery before claiming turnkey remote OAuth support.
- Existing OIDC `(issuer, subject)` identity and grant checks remain the source
  of database authorization; OAuth client registration must not grant database
  access by itself.
- Client setup docs and UI must clearly distinguish remote HTTP OAuth from
  stdio's local authentication behavior and explain unsupported client features.
- The initial delivery must include protocol-boundary integration tests with a
  local fake OIDC provider, alongside the repository's normal quality gate.

## Revisit when

Revisit if the OIDC provider can natively provide interoperable MCP OAuth
registration and consent without custom gateway authorization endpoints, or if
the MCP authorization specification changes in a way that affects the broker.
