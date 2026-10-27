# ADR 0003: Independent NATS and Valkey MCP services

- Status: Accepted
- Date: 2026-10-26
- Scope: remove NATS and Valkey runtime ownership from SQL MCP Gateway.

## Decision

NATS and Valkey each live in a separate repository and deployable service. Each service
owns its HTTP/MCP process, administration API and UI, migrations, control database,
target catalog, grants, and audit events. SQL MCP Gateway remains PostgreSQL/MySQL-only
and exposes only the SQL MCP resource.

OIDC and Vault remain external integrations configured per service. Operators may share
an OIDC provider or Vault deployment, but each app has independent audience/public URL,
database, credentials, policies, and release lifecycle. Neither service calls the SQL
gateway at runtime.

## Consequences

- Changes and releases to NATS or Valkey no longer require building or deploying the SQL
  gateway.
- Operators provision a separate control database for each service and configure its
  own environment, identity audience, Vault policies, and public origin.
- Existing integration metadata and grants remain in the legacy SQL database; the code
  change does not delete or migrate that data. A separately reviewed migration is
  required before retiring old records or Vault paths.
- Local SQL gateway compose and integration stacks no longer start NATS or Valkey.

## Supersedes

This decision supersedes the shared-deployment language in
[`streaming-and-cache-mcps.md`](../architecture/streaming-and-cache-mcps.md) and the
shared-runtime description in ADR 0002. ADR 0002 remains applicable to SQL MCP's
Streamable HTTP and remote stdio behavior only.
