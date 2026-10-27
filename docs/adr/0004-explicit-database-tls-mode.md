# ADR 0004: Explicit TLS mode per database target

- Status: Accepted
- Date: 2026-10-26
- Scope: PostgreSQL and MySQL target registration and query connections.

## Decision

TLS remains enabled and fully verified by default (`verify_full`). An administrator
may explicitly choose `disable` for an individual target when that database does
not support TLS. The choice is stored with the target and applied consistently to
Vault's connection verification and to query connections. It is never inferred
from a failed TLS handshake or silently downgraded. A TLS CA is valid only with
TLS enabled.

The admin UI must clearly warn that `disable` leaves database traffic unencrypted.
Database URLs do not carry TLS policy in query parameters; the target's explicit
`tls_mode` field is authoritative. Existing targets retain their stored mode.

## Consequences

- TLS-capable targets keep hostname and certificate validation by default.
- Operators can register legacy/private targets without TLS after explicitly
  accepting unencrypted transport for that target.
- Networks and database credentials for a no-TLS target require appropriate
  protection outside this gateway; the gateway does not claim transport
  confidentiality for that connection.
- Vault configuration and SQL client configuration must use the same stored
  mode so registration success does not mask query-time TLS failures.
