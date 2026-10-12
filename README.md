# SQL MCP Gateway

Self-hosted MCP gateway for PostgreSQL and MySQL, intended for deployment inside a private network or VPN. It provides Streamable HTTP and stdio transports, a SvelteKit administration UI, and per-user, per-database authorization.

## Security model

- OIDC-provider-agnostic identity validation using discovery and JWKS.
- Direct per-user grants: no access, `read`, or `write`; access is denied by default.
- `write` permits DML only. The gateway and the dynamic database identity both constrain database privileges.
- Vault brokers short-lived, distinct database identities. Gateway audit events correlate the OIDC issuer/subject with the database principal and a fingerprint of the Vault lease.
- Database root credentials are sent to Vault and are not persisted in the gateway control database.
- Vault administration and runtime access use separate tokens; the runtime identity only issues and revokes approved database leases.
- The gateway does not share authenticated database sessions between users or access modes.

## Repository layout

- `crates/gateway`: Rust HTTP/stdio gateway, OIDC, control-plane APIs, authorization, SQL adapters, and MCP protocol.
- `crates/vault-broker`: typed Vault database-secrets client.
- `crates/gateway/migrations`: control-plane PostgreSQL schema.
- `apps/admin`: SvelteKit administration UI.
- `docs/architecture.md`: product boundaries, security invariants, and implementation scope.

## Requirements

- Rust toolchain supporting edition 2024 and Cargo.
- PostgreSQL for gateway control-plane metadata.
- A reachable OIDC issuer with discovery and JWKS endpoints.
- Vault with the PostgreSQL and MySQL database secrets plugins enabled.
- Node.js and pnpm to develop or build the administration UI.
- SQLx CLI (`cargo install sqlx-cli --no-default-features --features postgres,rustls`) to manage migrations.
- Docker Compose v2 only for the isolated integration test stack.

## Configure and run the gateway

1. Create a private environment file and fill it using your local secret-management workflow:

   ```sh
   cp .env.example .env
   ```

   `.env` is ignored by Git. Set `CONTROL_DATABASE_URL` to a dedicated PostgreSQL control-plane database, configure the OIDC issuer/client/audience and at least one bootstrap administrator subject, and provide `VAULT_ADDRESS`, `VAULT_ADMIN_TOKEN`, and `VAULT_RUNTIME_TOKEN`. The Vault tokens must be distinct. Do not use database credentials, Vault tokens, or an OIDC token in command-line arguments.

2. Set `DATABASE_URL` in the environment to the same control-plane database used by `CONTROL_DATABASE_URL`, then run the migration commands from the repository root:

   ```sh
   make migrate-up
   make migrate-status
   ```

   SQLx CLI reads `DATABASE_URL`; the running gateway reads `CONTROL_DATABASE_URL`. Keep both pointed at the dedicated control-plane database, not a registered target database.

3. Start the gateway from the repository root (it loads `.env`):

   ```sh
   make run
   ```

   The default listener is `127.0.0.1:8080`. Health endpoints are `/health` and `/readyz`; the MCP endpoint is `/mcp`. The stdio mode uses the same server-side authorization and database adapters:

   ```sh
   make run-stdio
   ```

4. In another terminal, start the admin UI:

   ```sh
   make admin-dev
   ```

   Vite serves the UI locally and proxies `/v1` and `/mcp` to `127.0.0.1:8080`. The UI obtains OIDC configuration from the gateway. Production deployments must provide TLS and route the UI plus its `/v1` API requests through the deployment's trusted reverse proxy; do not expose the gateway or databases directly to the public internet.

## Local integration test

The opt-in integration suite exercises the control-plane migration, the real Vault database secrets engine, and real PostgreSQL/MySQL servers. It checks that Vault issues distinct dynamic users, read roles cannot mutate data, write roles can perform DML, DDL stays denied, and revoked credentials cannot reconnect.

Run it with:

```sh
make test-integration
```

The script requires Docker Compose and OpenSSL. It generates temporary test-only credentials and a local CA under `/tmp/opencode`, publishes service ports on loopback only, creates a Vault runtime token limited to issuing/revoking database leases, runs the integration test twice, then removes the uniquely named containers, test volumes, and temporary credentials. It does not use `.env`, external database URLs, or production/staging services. The fixed container image versions are declared in `compose.integration.yaml`.

## Quality checks

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd apps/admin && pnpm check && pnpm test && pnpm lint && pnpm build
```

Use `make test-integration` separately when validating Vault and database-engine behavior.
