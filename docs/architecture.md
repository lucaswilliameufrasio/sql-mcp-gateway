# Product and architecture plan

Status: product boundaries and identity/credential decisions accepted; the first end-to-end implementation is present. The local Vault/PostgreSQL/MySQL integration suite validates the dynamic-credential and database-privilege path. Environment-specific deployment and hardening gates are tracked in [production-readiness.md](production-readiness.md); deployment examples are indexed in [deployment guides](deployment/README.md).

## Product boundary

SQL MCP Gateway is a self-hosted, multi-user database access gateway for AI clients. It is separate from SQL Manager: SQL Manager is a human-operated desktop SQL client; this service authenticates users, enforces centrally managed grants, brokers database credentials, and exposes MCP tools.

The deployment belongs inside the private network or VPN that can reach the target databases. The databases themselves are not exposed to MCP clients or the public internet.

## Actors and responsibilities

- **Platform administrator:** registers PostgreSQL/MySQL targets, configures the Vault credential-broker references, and grants individual users read and/or write access to each target.
- **Database user:** authenticates through an OIDC provider, sees only authorized targets, and invokes MCP operations using a short-lived database identity associated with that session.
- **OIDC provider:** authenticates the human and supplies a stable issuer/subject identity. The gateway uses provider discovery, token validation, and the configured client/audience; platform operators configure those settings.
- **Vault:** stores database root connection secrets and issues short-lived, target- and access-mode-specific database credentials.
- **MCP client:** uses authenticated JSON-RPC `POST /mcp` or the local stdio process. The current HTTP endpoint is request/response JSON and does not implement Streamable HTTP SSE or GET sessions; stdio requires the host to inject an access token.
- **Implementation stack:** Rust for the gateway and SvelteKit for the admin web application.

## Main journeys

### Register a database

1. An administrator selects PostgreSQL or MySQL and enters endpoint, database/schema, a provisioner username/password, and an optional private TLS CA.
2. The gateway configures the Vault database connection and target-specific read/write roles. The database credential is write-only and is not persisted in the control-plane database or returned after submission.
3. The gateway stores target metadata and Vault role references in the control database. Registration fails if Vault setup or the metadata write fails.
4. The administrator grants access to selected OIDC users. Unassigned users have no access by default.

### Use a database

1. For remote HTTP, the MCP host uses the gateway's OAuth authorization server, which authenticates the person through the configured OIDC issuer and issues a gateway-audience token. A current OIDC access token remains supported as a compatibility path. For stdio, `--login` obtains the same gateway OAuth token interactively and stores its renewable credentials in the OS keyring; `SQL_MCP_ACCESS_TOKEN` remains a deprecated compatibility path.
2. The gateway resolves the issuer/subject, evaluates that user's grant for the requested target, and rejects missing or insufficient grants.
3. The credential broker obtains a short-lived Vault lease for that target and access mode.
4. The database connector opens a session using the unique temporary database identity. It never shares an authenticated session across users or access modes.
5. The gateway applies query, duration, and result limits; records an audit event linking the OIDC principal, target, Vault lease/database principal, operation, and outcome; and returns the result.
6. Credentials and database sessions are closed/revoked when possible at session end; Vault TTL is the recovery boundary if a client disconnects unexpectedly.

### Use the stdio transport

The stdio process is an HTTP bridge to the remote gateway; it does not open the control database or Vault and needs no operator settings. Run `sql-mcp-gateway --login <gateway-url>` to complete the gateway OAuth authorization-code/PKCE flow in a browser, then `sql-mcp-gateway --stdio <gateway-url>`. Omitting the URL uses `MCP_PUBLIC_URL`. Access and rotating refresh tokens are stored in the operating system credential store, not MCP configuration. Stdio refreshes before expiry and serializes refreshes within the process; an uncertain refresh failure requires reauthentication, not reuse. `--logout <gateway-url>` revokes the token family when possible and removes local credentials. If the OS credential store is unavailable, login fails closed with no plaintext-file fallback. Deprecated `SQL_MCP_ACCESS_TOKEN` remains supported for migration only, without automatic refresh. The bridge preserves JSON-RPC IDs and notifications, reads finite JSON/SSE responses, releases the session on EOF, and never replays a failed SQL request.

### Streamable HTTP session lifecycle

`/mcp` accepts authenticated POST, GET SSE and DELETE. Initialization creates a random UUID session bound to the authenticated principal in PostgreSQL; readiness requires `notifications/initialized`. Subsequent calls carry the session ID and negotiated protocol version. Sessions expire after 30 minutes; a missing session is 400, unknown/expired/foreign sessions are 404. Initialization admits at most 16 live sessions per principal using a PostgreSQL transaction/advisory lock. Expired rows are cleaned up at admission. DELETE ends the session; existing GET streams notice deletion/revocation within their five-second check interval.

POST accepts both JSON and SSE media types, normally returns JSON, and can return a finite SSE response when SSE is listed first. GET streams are finite, use comments as idle heartbeats, and acquire one 60-second stream lease per session across replicas. There are no unsolicited tool notifications (`listChanged: false`), replay IDs or persisted request results. GET reconnects do not replay SQL operations; a lost response has an uncertain outcome. All methods validate browser Origin against the configured public origin, and all requests authenticate independently of session IDs. Session metadata works across replicas; SSE connections themselves are local and bounded. Disable reverse-proxy buffering. See [ADR 0002](adr/0002-remote-stdio-and-streamable-http.md).

## Capability boundaries

- **Administration:** database target lifecycle, signed-in principal discovery, per-user grants, and audit review. OIDC and Vault endpoints/credentials are operator-managed runtime configuration.
- **Identity:** OIDC discovery and signed-token/userinfo validation; issuer plus subject is the stable principal key. Audience is validated locally and configured at the IdP, not sent upstream as a `resource` indicator. MCP's gateway resource binding remains distinct. The implementation uses standard identity claims rather than configurable claim mapping.
- **Authorization:** deny by default; grants are scoped to a user and a target, with `read` and `write` levels. Every MCP operation is authorized server-side, including listing and schema discovery.
- **Credential brokering:** converts an authorized principal/target/access mode into a Vault lease. Target and role references come from server-owned records, never untrusted MCP arguments.
- **Database adapters:** PostgreSQL and MySQL connection, schema discovery, query execution, timeout/cancellation, and safe result shaping.
- **MCP transports:** Streamable HTTP with PostgreSQL sessions, POST JSON/SSE, GET idle SSE and DELETE; stdio bridges to the same authenticated endpoint rather than duplicating query execution.
- **Audit:** append-only security events with actor, target, access mode, Vault lease/database principal, operation class, duration, and outcome. Do not log credentials or result values by default.

## Authorization invariants

- Effective access is the intersection of the gateway grant and the database privileges attached to the Vault-issued identity. The gateway can narrow database privileges, never increase them.
- A write grant is implemented with a distinct Vault/database role, not only by parsing SQL in the gateway.
- The initial safe interpretation of `write` is data mutation without schema/role/database administration. DDL and privilege-management operations remain denied unless deliberately designed as a separate capability.
- Users without a grant cannot enumerate a target, and each tool call rechecks the grant to handle revocation during a session.
- Connection pools are partitioned by target, Vault lease/database identity, and access mode. A connection authenticated for one user or mode is never reused for another.
- No MCP tool accepts arbitrary hostnames, connection strings, Vault paths, or role names from the caller.

## Baseline implementation defaults

- Control-plane metadata is stored in a dedicated PostgreSQL database, separate from registered target databases.
- Users are identified by the OIDC `(issuer, subject)` pair. The first version grants database access directly to users; groups can be added after the individual grant lifecycle is complete.
- Platform roles are `admin` and `member`; database privileges are independent, per-user grants (`read` or `write`).
- `write` means DML (`INSERT`, `UPDATE`, `DELETE`) plus `SELECT`; DDL, privilege changes, and database administration remain unavailable.
- Vault uses separate administration and runtime identities. The administration identity configures connections/roles; the runtime identity can only issue/revoke approved dynamic roles.
- Vault-issued leases are per database session, have bounded TTL, and are revoked on normal completion; TTL is the crash/disconnect fallback.
- API error responses use a stable PT-BR error envelope; routes are versioned under `/v1` except health/readiness.

## Proposed module/data ownership

- **Control plane:** owns target metadata, principal identities, user grants, and audit metadata. It stores Vault references rather than database passwords.
- **Vault integration:** owns secret writes and credential issuance. A broker interface allows additional secret managers or identity mechanisms later without weakening the authorization contract.
- **Database adapters:** own protocol-specific connection/session behavior. PostgreSQL and MySQL differences remain behind typed adapter contracts.
- **MCP layer:** owns protocol serialization and tool schemas only; it does not contain database authorization rules or direct Vault policy decisions.

## Implementation slices

1. **Credential-broker proof:** validate short-lived, unique, least-privilege credentials and revocation/TTL for both PostgreSQL and MySQL using Vault's database engine. Verify the gateway can correlate leases to OIDC principals without exposing secrets.
2. **Control-plane foundation:** persist target metadata and per-user grants; add admin APIs/UI for target registration and grants; write database root secrets only to Vault.
3. **OIDC and authorization:** validate tokens from a configurable issuer, map issuer/subject and optional groups, and cover allowed/denied cases with integration tests.
4. **Remote read-only MCP journey:** expose authorized target listing, schema inspection, and bounded read queries over authenticated HTTP POST with JSON request/response.
5. **Write journey:** add separate Vault/database write roles and explicitly scoped DML operations, with authorization and audit regression tests.
6. **stdio bridge:** relay to the remote gateway authorization policy with browser OAuth/PKCE login, keyring credential storage and automatic refresh; no client-side operator secrets.
7. **Hardening:** test revocation, session expiry, Vault/database outages, query cancellation, audit completeness, TLS failures, and concurrent sessions.

## Decisions deferred behind a working vertical slice

- OIDC provider-specific claim mapping, beyond `iss` and `sub`.
- Additional group-based grants and custom role composition.
- Vault auto-auth methods beyond the first self-hosted deployment configuration.

The opt-in `make test-integration` suite runs twice against isolated Vault, PostgreSQL, MySQL and Keycloak services. It validates dynamic credentials/DML permissions, real browser OIDC login and signed JWT/userinfo validation, persisted OAuth codes and rotation/replay/revocation, current grants, Streamable HTTP session isolation/lifecycle/SSE and the actual remote stdio subprocess without operator configuration. This proves the isolated integration contract, not production-specific TLS, network, IdP policy or operational hardening; those require deployment-specific evidence using the [release checklist](production-readiness.md).

## External contracts checked

- The official Rust MCP SDK is `rmcp` and documents both stdio and Streamable HTTP transports: <https://github.com/modelcontextprotocol/rust-sdk>.
- Vault's database secrets engine provides database connection/role configuration, dynamic credential issuance, and lease-based revocation. Its PostgreSQL and MySQL plugins need separate role statements and validation: <https://developer.hashicorp.com/vault/docs/secrets/databases> and <https://developer.hashicorp.com/vault/api-docs/secret/databases>.
