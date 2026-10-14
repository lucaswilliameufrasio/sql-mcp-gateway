# SQL MCP Gateway

Self-hosted gateway for giving authenticated people controlled access to PostgreSQL and MySQL through MCP. Deploy it inside a private network or VPN. The gateway validates OIDC access tokens, applies per-person/per-database grants, and asks Vault for short-lived database identities.

The HTTP endpoint currently handles authenticated JSON-RPC requests with `POST /mcp`; it does not implement Streamable HTTP SSE or GET sessions. The administration UI and gateway are separate processes. See [architecture and protocol boundaries](docs/architecture.md) before choosing an MCP client or deployment shape.

## Choose a path

| Goal                                                               | Start here                                                     |
| ------------------------------------------------------------------ | -------------------------------------------------------------- |
| Run the gateway and admin UI locally                               | [Local first run](#local-first-run)                            |
| Deploy on a VPS, with or without an existing Caddy/Nginx           | [VPS deployment](docs/deployment/vps.md)                       |
| Deploy the gateway on Amazon ECS Managed Instances                 | [ECS deployment](docs/deployment/ecs-managed-instances.md)     |
| Deploy the gateway on Lambda with a private API and RDS PostgreSQL | [Lambda deployment](docs/deployment/lambda-web-adapter-rds.md) |
| Check production gates                                             | [Production readiness](docs/production-readiness.md)           |

## Local first run

This is not a one-command demo: the gateway needs a control-plane PostgreSQL database, an OIDC provider, and Vault configured with the database secrets engines. It also needs private-network access to each database you register. These services are intentionally not started with the app.

### 1. Prepare the local project

Install the stable Rust toolchain (edition 2024), Node.js, and pnpm, then run `make setup` from the repository root:

```sh
make setup
```

The target fetches the locked Rust dependencies, installs the SQLx CLI if needed, installs the admin UI dependencies, and creates `.env` from `.env.example` only if `.env` does not already exist. It does not install Rust, Node.js, or pnpm, and it does not start or provision PostgreSQL, OIDC, or Vault.

### 2. Configure local services

Review `.env` and fill it using your local secret-management workflow. `make setup` never overwrites an existing `.env`.

Set `CONTROL_DATABASE_URL` and `DATABASE_URL` to the **same dedicated PostgreSQL control-plane database**. The gateway reads `CONTROL_DATABASE_URL`; SQLx migrations use `DATABASE_URL`. Do not point either one at a registered target database.

Also set `OIDC_ISSUER_URL`, `OIDC_CLIENT_ID`, `OIDC_ACCESS_TOKEN_AUDIENCE`, and `OIDC_BOOTSTRAP_ADMIN_SUBJECTS` (the exact OIDC `sub` value for the first administrator; separate multiple subjects with commas), plus `VAULT_ADDRESS`, `VAULT_ADMIN_TOKEN`, and `VAULT_RUNTIME_TOKEN`. The Vault tokens must be distinct; the runtime token should only issue and revoke approved database leases. The app loads `.env` for local runs, and SQLx reads `DATABASE_URL` from it for the migration targets below. `.env` is ignored by Git; never commit resolved credentials.

For local development only, the gateway permits plain HTTP OIDC/Vault URLs on loopback addresses such as `http://127.0.0.1:8200`. Remote OIDC and Vault endpoints must use HTTPS. Configure the OIDC client with this exact development redirect URI:

```text
http://127.0.0.1:5173/auth/callback
```

### 3. Create/update the control-plane schema

From the repository root:

```sh
make migrate-up
make migrate-status
```

The gateway also applies embedded migrations at startup. The explicit migration step lets you check connectivity and schema state before starting the processes.

### 4. Start the gateway and admin UI

In terminal 1, from the repository root:

```sh
make run
```

In terminal 2:

```sh
make admin-dev
```

Open <http://127.0.0.1:5173>. Check the gateway's unauthenticated health endpoints; both return HTTP `204` when healthy (readiness checks the control database only):

```sh
curl -i http://127.0.0.1:8080/health
curl -i http://127.0.0.1:8080/readyz
```

### 5. Sign in, register a database, and grant access

1. Sign in through the configured OIDC provider using the subject listed in `OIDC_BOOTSTRAP_ADMIN_SUBJECTS`. Its first authenticated request creates the principal and bootstraps it as a platform administrator.
2. In **Databases**, register a reachable PostgreSQL or MySQL target. The gateway validates the connection and sends the supplied database administrator credential to Vault; it does not retain that password in its control database.
3. Each user must sign in at least once so the gateway can create their principal. In **Access**, select that person and database, then grant `read` or `write`. No database access is granted by default; `write` includes read access and adds DML, but not DDL.

### 6. Connect an MCP client

Configure the client to send authenticated JSON-RPC `POST` requests to:

```text
http://127.0.0.1:8080/mcp
```

It must send an OIDC **access token** whose audience matches `OIDC_ACCESS_TOKEN_AUDIENCE` as `Authorization: Bearer …`, with `Content-Type: application/json` and `Accept: application/json`. Use the deployment's HTTPS origin outside local development. Confirm that the client supports this request/response HTTP transport; full Streamable HTTP sessions/SSE are not implemented here.

The alternative stdio mode is `make run-stdio`. The MCP client must inject a current OIDC access token as `SQL_MCP_ACCESS_TOKEN` into that process; do not save a long-lived token in the repository or a shared shell command.

## Deploy

Deployment is environment-specific: the examples do not create the control database, Vault, target databases, identity provider, or (for AWS) the shared VPC/network. Follow the ordered instructions and validate the plan in your own environment:

1. [Deployment guide index](docs/deployment/README.md) — choose a target and prepare shared dependencies.
2. Follow the target guide for build/package, secret references, routing, and health checks.
3. Review the [production readiness checklist](docs/production-readiness.md) before enabling users.

The OpenTofu examples are starting points, not validated production modules. Their operator-editable `locals.auto.tf` files are copied from ignored example templates. Review IAM, networking, database connection budgets, costs, and the complete plan before you choose to apply anything. This repository has not deployed these examples to an AWS account.

## Security model

- OIDC identity is validated using provider discovery and JWKS. Platform administrator bootstrap matches exact OIDC subjects.
- Access is denied by default and granted per person and database as `read` or `write`.
- `read` accepts a single `SELECT`; `write` includes `read` and adds `INSERT`, `UPDATE`, and `DELETE`. The gateway and the short-lived database identity both constrain privileges; DDL stays blocked.
- Vault uses separate administration and runtime credentials. Database root credentials are sent to Vault and are not persisted in the gateway control database.
- Database sessions are not shared across users or access modes. Audit events correlate the OIDC issuer/subject with the database principal and Vault lease fingerprint.
- Keep the gateway, Vault, and target databases on private routes. VPN access does not replace OIDC or per-user grants.

## Repository map

- `crates/gateway`: Rust HTTP/stdio gateway, OIDC, control-plane API, authorization, SQL adapters, and MCP protocol.
- `crates/vault-broker`: typed client for Vault database credentials.
- `crates/gateway/migrations`: control-plane PostgreSQL schema.
- `apps/admin`: SvelteKit administration UI.
- `docs/architecture.md`: product boundaries and security invariants.
- `docs/deployment/`: deployment procedures and OpenTofu examples.

## Tests and quality checks

The regular local checks are:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd apps/admin && pnpm check && pnpm test && pnpm lint && pnpm build
```

`make test-integration` is separate and starts an isolated Docker Compose stack for Vault, PostgreSQL, and MySQL. It is not required to run the app locally; read the script and its cleanup behavior before opting in.

To opt in, run `make test-integration` from the repository root. The script generates test-only credentials and local TLS files under `/tmp/opencode`, binds service ports to loopback, uses fixed image versions from `compose.integration.yaml`, and removes its uniquely named containers, test volumes, and temporary credentials when it finishes. It does not use `.env` or connect to external databases.
