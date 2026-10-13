# Product and architecture plan

Status: product boundaries and identity/credential decisions accepted; the first end-to-end implementation is present. The local Vault/PostgreSQL/MySQL integration suite validates the dynamic-credential and database-privilege path. Environment-specific deployment and hardening gates are tracked in [production-readiness.md](production-readiness.md); deployment examples are indexed in [deployment guides](deployment/README.md).

## Product boundary

SQL MCP Gateway is a self-hosted, multi-user database access gateway for AI clients. It is separate from SQL Manager: SQL Manager is a human-operated desktop SQL client; this service authenticates users, enforces centrally managed grants, brokers database credentials, and exposes MCP tools.

The deployment belongs inside the private network or VPN that can reach the target databases. The databases themselves are not exposed to MCP clients or the public internet.

## Actors and responsibilities

- **Platform administrator:** registers PostgreSQL/MySQL targets, configures the Vault credential-broker references, and grants individual users read and/or write access to each target.
- **Database user:** authenticates through an OIDC provider, sees only authorized targets, and invokes MCP operations using a short-lived database identity associated with that session.
- **OIDC provider:** authenticates the human and supplies a stable issuer/subject identity. Provider-specific behavior is isolated behind standard discovery, token validation, and configurable claim mapping.
- **Vault:** stores database root connection secrets and issues short-lived, target- and access-mode-specific database credentials.
- **MCP client:** connects through remote Streamable HTTP or a local stdio bridge.
- **Implementation stack:** Rust for the gateway and SvelteKit for the admin web application.

## Main journeys

### Register a database

1. An administrator selects PostgreSQL or MySQL and enters endpoint, TLS, and Vault database-engine settings.
2. The service configures the target in Vault; secret fields are write-only and are never persisted in the control-plane database or returned after submission.
3. The administrator associates the target with Vault roles for read and, optionally, write credentials, then validates connectivity.
4. The administrator grants access to selected OIDC users. Unassigned users have no access by default.

### Use a database

1. The MCP client authenticates the person with the configured OIDC issuer.
2. The gateway resolves the issuer/subject, evaluates that user's grant for the requested target, and rejects missing or insufficient grants.
3. The credential broker obtains a short-lived Vault lease for that target and access mode.
4. The database connector opens a session using the unique temporary database identity. It never shares an authenticated session across users or access modes.
5. The gateway applies query, duration, and result limits; records an audit event linking the OIDC principal, target, Vault lease/database principal, operation, and outcome; and returns the result.
6. Credentials and database sessions are closed/revoked when possible at session end; Vault TTL is the recovery boundary if a client disconnects unexpectedly.

### Use the stdio transport

The local stdio process is an authenticated bridge to the same self-hosted gateway, not a second authorization or database-credential implementation. It obtains a user-scoped login through a browser/device flow and forwards MCP operations to the gateway. Tokens are stored only in an OS credential store where supported, never in MCP configuration or process arguments.

## Capability boundaries

- **Administration:** database target lifecycle, OIDC claim mapping, user grants, Vault references, and connectivity checks.
- **Identity:** OIDC discovery and token validation; issuer plus subject is the stable principal key. Claim mapping is configuration, not provider-specific business logic.
- **Authorization:** deny by default; grants are scoped to a user and a target, with `read` and `write` levels. Every MCP operation is authorized server-side, including listing and schema discovery.
- **Credential brokering:** converts an authorized principal/target/access mode into a Vault lease. Target and role references come from server-owned records, never untrusted MCP arguments.
- **Database adapters:** PostgreSQL and MySQL connection, schema discovery, query execution, timeout/cancellation, and safe result shaping.
- **MCP transports:** remote Streamable HTTP and stdio bridge; both call the same application services and authorization policy.
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

- **Control plane:** owns target metadata, OIDC issuer/claim configuration, user grants, and audit metadata. It stores Vault references rather than database passwords.
- **Vault integration:** owns secret writes and credential issuance. A broker interface allows additional secret managers or identity mechanisms later without weakening the authorization contract.
- **Database adapters:** own protocol-specific connection/session behavior. PostgreSQL and MySQL differences remain behind typed adapter contracts.
- **MCP layer:** owns protocol serialization and tool schemas only; it does not contain database authorization rules or direct Vault policy decisions.

## Implementation slices

1. **Credential-broker proof:** validate short-lived, unique, least-privilege credentials and revocation/TTL for both PostgreSQL and MySQL using Vault's database engine. Verify the gateway can correlate leases to OIDC principals without exposing secrets.
2. **Control-plane foundation:** persist target metadata and per-user grants; add admin APIs/UI for target registration and grants; write database root secrets only to Vault.
3. **OIDC and authorization:** validate tokens from a configurable issuer, map issuer/subject and optional groups, and cover allowed/denied cases with integration tests.
4. **Remote read-only MCP journey:** expose authorized target listing, schema inspection, and bounded read queries over Streamable HTTP.
5. **Write journey:** add separate Vault/database write roles and explicitly scoped DML operations, with authorization and audit regression tests.
6. **stdio bridge:** add local login and relay to the same gateway; verify it has no alternate path around central policy.
7. **Hardening:** test revocation, session expiry, Vault/database outages, query cancellation, audit completeness, TLS failures, and concurrent sessions.

## Decisions deferred behind a working vertical slice

- OIDC provider-specific claim mapping, beyond `iss` and `sub`.
- Additional group-based grants and custom role composition.
- Vault auto-auth methods beyond the first self-hosted deployment configuration.

The credential broker, control plane, OIDC authorization, SQL adapters, admin UI, and HTTP/stdio MCP paths have an initial implementation. The opt-in `make test-integration` suite starts isolated local Vault, PostgreSQL, and MySQL services and validates the real dynamic-credential, DML privilege, uniqueness, and revocation behavior. This proves the local integration contract, not production-specific TLS, network, identity-provider, or operational hardening; those require deployment-specific evidence using the [release checklist](production-readiness.md).

## External contracts checked

- The official Rust MCP SDK is `rmcp` and documents both stdio and Streamable HTTP transports: <https://github.com/modelcontextprotocol/rust-sdk>.
- Vault's database secrets engine provides database connection/role configuration, dynamic credential issuance, and lease-based revocation. Its PostgreSQL and MySQL plugins need separate role statements and validation: <https://developer.hashicorp.com/vault/docs/secrets/databases> and <https://developer.hashicorp.com/vault/api-docs/secret/databases>.
