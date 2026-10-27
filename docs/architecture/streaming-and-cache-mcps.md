# NATS and Valkey MCPs

Status: extracted from the SQL MCP Gateway into independent projects.

## Ownership and runtime boundaries

- [`nats-mcp`](../../../nats-mcp) owns the NATS/JetStream MCP process, HTTP/admin API,
  admin UI, control-plane migrations, targets, grants, and audit events.
- [`valkey-mcp`](../../../valkey-mcp) owns the Valkey MCP process, HTTP/admin API,
  admin UI, control-plane migrations, targets, grants, and audit events.
- SQL MCP Gateway owns only PostgreSQL/MySQL access and exposes only `/mcp`.
- Each service has its own repository, binary, build/test workflow, deployment, public
  origin, and control database. OIDC and Vault are external dependencies and are
  configured independently per deployment; sharing their infrastructure is optional,
  not a runtime dependency on SQL MCP Gateway.
- Service-specific endpoints are `/nats/mcp` and `/valkey/mcp` relative to that
  service's configured `MCP_PUBLIC_URL`.

## Security and data ownership

The MCP services keep upstream credentials in Vault, target metadata and grants in
their own PostgreSQL control databases, and reject access by default. Tool calls resolve
only registered targets, recheck grants, enforce resource scopes, bound results, and
record audit metadata without recording credentials or returned payloads.

The SQL gateway's historical migration and records for integration targets are retained
to avoid destructive data removal. They are no longer served by the SQL gateway. New
MCP deployments start with their own empty control databases; existing target/grant data
and Vault references are not copied automatically. Plan a separately reviewed data
migration before retiring any legacy records that still matter.

## Tool scope

- **NATS:** list authorized JetStream streams, read/delete a selected message by
  stream/sequence, and publish only to authorized subjects. No consumer administration
  or stream-wide deletion.
- **Valkey:** scan keys incrementally within an authorized prefix; inspect bounded string
  values/TTL; set values with a required bounded TTL; delete explicit authorized keys.
  No arbitrary commands, `KEYS`, or server-wide destructive operations.

Writes and deletes require explicit write grants. MCP arguments cannot provide arbitrary
hosts or credentials.

## Local verification

Each repository's README lists its independent Rust and admin UI checks. Real broker,
Vault, PostgreSQL, and OIDC integration tests belong to the corresponding MCP project;
they are not part of the SQL gateway's integration lifecycle.
