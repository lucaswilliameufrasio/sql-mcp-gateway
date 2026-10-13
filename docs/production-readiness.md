# Production readiness: private network / VPN

This checklist turns the accepted deployment boundary into operator-verifiable
release gates. The gateway belongs inside the private network that can reach
Vault and the registered PostgreSQL/MySQL targets. A VPN is a network boundary,
not a replacement for OIDC authentication or per-user database grants.

Choose and adapt a deployment model using the [deployment guides](deployment/README.md).
Their examples are starting points, not proof that a target environment is
production-ready.

No VPN provider, production host, identity provider, or Vault instance is
selected by this repository. Record those environment-specific choices during
deployment. Never put resolved tokens, database passwords, private keys, or
`.env` contents in this document or Git.

## Network and transport

- [ ] Place the gateway, Vault, and target databases on private routes. Target
      databases must not be reachable directly from MCP clients or the public
      internet.
- [ ] Limit gateway and admin UI access to approved VPN/private-network peers.
      Restrict VPN membership/ACLs and remove access when a peer is no longer
      authorized.
- [ ] Keep the gateway's default listener (`127.0.0.1:8080`) behind a local
      reverse proxy where possible. If components run in separate containers,
      bind only to their private service network. Never publish the gateway
      listener publicly; restrict any direct VPN-interface listener at the
      firewall to approved VPN sources.
- [ ] Terminate TLS at the trusted reverse proxy for the admin UI and remote
      MCP clients. Route the UI, `/v1`, and `/mcp` through the intended origin;
      do not expose the gateway's plain-HTTP listener outside its private
      service boundary.
- [ ] Restrict `/health`, `/readyz`, and `/v1/auth/config` to the intended
      monitor/proxy network; all three are unauthenticated. `/readyz` checks the
      control-plane database only, not Vault or every registered target.
- [ ] Limit gateway egress to the configured OIDC endpoints, control-plane
      PostgreSQL, Vault, registered PostgreSQL/MySQL targets, and explicitly
      approved operational telemetry.

## Identity and authorization

- [ ] Configure the real OIDC issuer, client ID, and access-token audience.
      The production issuer must use HTTPS and provide trusted discovery, JWKS,
      and userinfo endpoints.
- [ ] Verify each bootstrap administrator's exact OIDC subject before adding it
      to `OIDC_BOOTSTRAP_ADMIN_SUBJECTS`. Do not treat VPN membership or email
      alone as application authorization.
- [ ] Confirm ungranted users cannot enumerate targets. Grant `read` or `write`
      only per verified user and per database; remove grants when no longer
      needed.
- [ ] Verify read grants cannot mutate data. Verify write grants allow only the
      intended DML and cannot perform DDL, privilege changes, or database
      administration.
- [ ] Revoke a grant while a client session is active and verify its next
      operation is denied. Both HTTP and stdio paths must retain the same
      server-side authorization policy.

## Vault and database targets

- [ ] Use a trusted TLS endpoint for Vault. Supply `VAULT_ADMIN_TOKEN` and
      `VAULT_RUNTIME_TOKEN` from the deployment secret manager; keep them
      distinct and out of source control, logs, command arguments, and tickets.
- [ ] Restrict the runtime Vault policy to issuing and revoking only approved
      database roles/leases. Keep Vault administration separate from gateway
      runtime access.
- [ ] Configure PostgreSQL and MySQL targets and their read/write Vault roles
      separately. Database identities must be least-privilege; write identities
      must not receive DDL, role-management, or database-administration rights.
- [ ] Store target root credentials in Vault, not gateway metadata. Submit
      credentials through the admin flow's write-only secret fields.
- [ ] Configure and verify database TLS certificate and hostname validation
      for both engines; invalid or untrusted certificates must fail closed.
- [ ] Verify leases use bounded TTLs, are unique for their intended session and
      access mode, and are revoked after normal completion. TTL must bound
      credentials left behind by crashes or network loss.

## Operations and recovery

- [ ] Point `CONTROL_DATABASE_URL` at a dedicated metadata database, separate
      from all registered targets. Restrict its service account to the gateway's
      required runtime and migration privileges.
- [ ] Back up the control-plane database and Vault state/configuration using
      their supported procedures. Encrypt off-host copies, restrict access, and
      perform an isolated restore drill; restoring only the control database
      may leave its Vault references unusable.
- [ ] Define audit retention and access. Preserve actor, target, access mode,
      database principal/lease fingerprint, operation class, duration, and
      outcome; do not log credentials or query results by default.
- [ ] Monitor gateway health/readiness, control-plane connectivity, OIDC and
      Vault availability, lease issue/revoke failures, target TLS/connectivity,
      query timeouts, and storage capacity. Assign alert owners.
- [ ] Review `MAX_QUERY_ROWS` and `MAX_QUERY_DURATION_SECONDS` against the
      expected workload without weakening bounds to mask unbounded queries.
- [ ] Name owners and document secret rotation, break-glass access, upgrade and
      rollback, backup restore, and incident response procedures.

## Release acceptance evidence

Record evidence from the actual deployment before enabling real users. The
repository's local integration suite is necessary but does not replace these
environment checks.

| Gate | Evidence required |
| --- | --- |
| VPN isolation | Approved VPN peer reaches the TLS endpoint; a non-VPN peer cannot connect |
| OIDC | Real issuer works; wrong issuer/audience and expired tokens are rejected |
| Grants | Unassigned user cannot enumerate targets; read/write grants apply only to their target and mode |
| Vault | Runtime policy is limited; leases are unique, short-lived, and revocable |
| PostgreSQL/MySQL TLS | Both engines validate CA and hostname; invalid certificates fail closed |
| SQL privileges | Read cannot mutate; write permits intended DML but denies DDL/admin operations |
| Revocation | Grant removal denies the next request; expired/revoked database leases cannot be reused |
| Recovery | Control-plane and Vault restore succeeds in an isolated environment |
| Operations | Monitoring, audit retention, rotation, and rollback owners are recorded |

`make test-integration` exercises disposable local Vault, PostgreSQL, and MySQL
services and has passed twice according to the project validation record. It
starts Docker services, creates temporary volumes, and removes them afterward.
It was not repeated for this documentation change. Schedule it explicitly when
disk-intensive local testing is acceptable; it does not validate production
VPN, identity-provider, TLS, or recovery configuration.
