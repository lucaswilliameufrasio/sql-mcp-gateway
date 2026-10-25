# ADR 0002: Remote stdio bridge and Streamable HTTP

- Status: Accepted
- Date: 2026-10-25
- Scope: user-requested completion of HTTP sessions/SSE, remote stdio and real OIDC tests.

## Decision

The gateway alone owns database/Vault credentials, grants and query execution.
`--stdio <gateway-url>` is an authenticated HTTP client, not a second gateway.
The existing `--stdio` form resolves `MCP_PUBLIC_URL` only. Login/keyring and
refresh behavior are shared; legacy environment tokens remain supported.

HTTP `/mcp` supports POST, GET SSE and DELETE. Initialization creates a random,
principal-bound PostgreSQL session, valid for 30 minutes. Sessions survive
replica changes but are not credentials: every request still authenticates.
Initialization is followed by `notifications/initialized`; subsequent requests
must carry the session and negotiated version. At most 16 live sessions per
principal are admitted. Browser Origin must match the public gateway origin.

For the MCP transport's backwards-compatibility rule, the negotiated version
stored in the session also identifies the version if a client omits the
protocol-version header. Present headers must be unambiguous, supported and
match the session; malformed or mismatched values are rejected on every method.

POST normally returns JSON, or a finite SSE response when explicitly preferred.
GET opens a bounded idle stream (the current tools advertise no unsolicited
notifications). It periodically checks session and credential validity. SSE
replay/event IDs are deliberately not offered: replaying a SQL write after a
disconnect is unsafe. The bridge never retries tool calls or writes; expired
sessions require the MCP host to initialize again. No request result is durable.

## Journeys and verification

1. A person logs in once, then a subprocess forwards MCP initialization and
   tool calls using keyring credentials; it needs no control DB, OIDC or Vault
   operator settings. Errors remain protocol messages on stdout, logs on stderr.
2. A remote client initializes, confirms readiness, calls tools, optionally
   listens on GET SSE, and deletes its session. Another principal cannot use it.
3. Isolated integration services include Keycloak (real discovery/JWKS, login,
   signed access tokens and userinfo), PostgreSQL and the real application router.
   Tests cover consent/PKCE/replay/rotation/revocation and transport authorization.

## Alternatives and consequences

Local sessions would require sticky routing and fail on replica changes; use
PostgreSQL instead. Keeping local stdio execution duplicates operator secrets
on every client and is rejected. Native keyring remains mandatory with no
plaintext fallback. GET streams are finite and bounded; proxies must disable
buffering and clients reconnect as needed. Revisit if server-initiated events,
durable replay, or a distributed cancellation protocol become requirements.
