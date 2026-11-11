# SQL MCP Gateway

Self-hosted gateway for giving authenticated people controlled access to PostgreSQL and MySQL through MCP. Deploy it inside a private network or VPN. The gateway validates OIDC access tokens, applies per-person/per-database grants, and asks Vault for short-lived database identities.

The gateway supports Streamable HTTP at `/mcp` (POST JSON/SSE, GET SSE and DELETE sessions), plus a remote stdio bridge. The administration UI and gateway are separate processes. See [architecture and protocol boundaries](docs/architecture.md) for lifecycle and deployment requirements.

## Choose a path

| Goal                                                               | Start here                                                     |
| ------------------------------------------------------------------ | -------------------------------------------------------------- |
| Run the gateway and admin UI locally                               | [Local first run](#local-first-run)                            |
| Deploy on a VPS, with or without an existing Caddy/Nginx           | [VPS deployment](docs/deployment/vps.md)                       |
| Deploy the gateway on Amazon ECS Managed Instances                 | [ECS deployment](docs/deployment/ecs-managed-instances.md)     |
| Deploy the gateway on Lambda with a private API and RDS PostgreSQL | [Lambda deployment](docs/deployment/lambda-web-adapter-rds.md) |
| Operate several environments (dev, staging, production)            | [Workspaces](#several-environments-workspaces)                 |
| Check production gates                                             | [Production readiness](docs/production-readiness.md)           |

## Local first run

The gateway needs a control-plane PostgreSQL database, an OIDC provider, and Vault configured with the database secrets engines. Choose the isolated local Compose stack for a development demo, or connect the host-run app to services you already operate.

### Option A: isolated local Compose stack

#### 1. Prepare the project and start local services

Install the stable Rust toolchain (edition 2024), Node.js, and pnpm, then run `make setup` from the repository root:

```sh
make setup
```

The target fetches the locked Rust dependencies, installs the SQLx CLI if needed, installs the admin UI dependencies, and creates `.env` from `.env.example` only if `.env` does not already exist. It does not install Rust, Node.js, or pnpm, and it does not start or provision PostgreSQL, OIDC, or Vault.

Start the separate development-only Compose services:

```sh
make dev-up
```

This creates `.env.dev` with owner-only permissions, `.dev/vault-runtime/runtime.env` for Vault's scoped runtime token, and `.dev/tls/` with local-only database certificates. These paths are ignored by Git; the existing `.env` is not read or overwritten. The stack uses Authentik, Vault dev mode, a dedicated control database, and TLS-enabled PostgreSQL/MySQL targets. It publishes Authentik, Vault, the gateway, and the admin UI only on `127.0.0.1`; data targets are reachable only inside the Compose network. The stack defaults to Docker's `rootless` context; set `DOCKER_CONTEXT` only if you deliberately selected another context.

#### 2. Authentik OIDC configuration (automatic)

The Authentik worker imports `deploy/authentik/sql-mcp-gateway-dev.yaml` from the Compose stack. It creates the `sql-mcp-gateway-dev` public Authorization Code application/provider, separate `nats-mcp-dev` and `valkey-mcp-dev` public providers for the standalone MCPs, and the `dev-admin` test user. The same local admin is assigned to all three applications; all use Authentik's asymmetric self-signed key and the `openid`, `profile`, and `email` scopes. The gateway and MCP admin subject is `dev-admin`.

The OIDC defaults are issuer `http://127.0.0.1:9000/application/o/sql-mcp-gateway-dev/`, client ID and audience `sql-mcp-gateway-dev`, and redirect URI `http://127.0.0.1:15173/auth/callback`. The browser client sends S256 PKCE; Authentik 2026.8 validates PKCE when sent but does not provide a server-side option to require it. The Authentik worker healthcheck waits for issuer discovery, and the gateway depends on that healthcheck. No Authentik UI setup is required.

The generated `.env.dev` contains separate local-only passwords for `akadmin` and `dev-admin`. Use the `dev-admin` password from that owner-only file when signing in to the gateway; never paste either password into chat or commit them.

#### 3. Start the gateway and admin UI

Start the gateway and admin UI together in Compose:

```sh
make dev-app
```

Open <http://127.0.0.1:15173> and sign in as `dev-admin`. `make dev-status` shows service status. Use `make dev-pause` and `make dev-resume` to pause/resume the stack without losing Vault's in-memory development state. `make admin-dev` remains available for the separate workflow that runs an admin UI on the host against services you already operate.

In **Databases**, register either sample target using the Compose service hostname and port: `target-postgres:5432` or `target-mysql:3306`. Use database `sql_mcp_gateway_dev`, the matching generated root credential from `.env.dev`, and the CA certificate at `.dev/tls/certs/ca.crt`. TLS verification is the default. For a database that does not support TLS, an administrator can explicitly select **Sem TLS** for that target; the UI warns that traffic is unencrypted, and the gateway does not silently downgrade. This Compose stack contains only the SQL gateway dependencies.

The stack is not production-ready. Vault runs in dev mode with an in-memory store, the local Authentik bootstrap password is stored in `.env.dev`, and database TLS uses a local CA. Recreating Vault loses its database role configuration; targets must be registered again. Avoid Compose volume-removal commands unless you intentionally want to delete local database data.

### Option B: use services you already operate

#### 1. Prepare the local project

Run `make setup` from the repository root after installing the stable Rust toolchain (edition 2024), Node.js, and pnpm:

```sh
make setup
```

This prepares repository dependencies and creates `.env` from `.env.example` only if `.env` does not already exist. It does not install language runtimes or provision services.

#### 2. Configure local services

Review `.env` and fill it using your local secret-management workflow. `make setup` never overwrites an existing `.env`.

Set `CONTROL_DATABASE_URL` and `DATABASE_URL` to the **same dedicated PostgreSQL control-plane database**. The gateway reads `CONTROL_DATABASE_URL`; SQLx migrations use `DATABASE_URL`. Do not point either one at a registered target database.

Also set `OIDC_ISSUER_URL`, `OIDC_CLIENT_ID`, `OIDC_ACCESS_TOKEN_AUDIENCE`, and `OIDC_BOOTSTRAP_ADMIN_SUBJECTS` (the exact OIDC `sub` value for the first administrator; separate multiple subjects with commas), plus `MCP_PUBLIC_URL` (the externally reachable HTTPS origin of the gateway) and a randomly generated `MCP_OAUTH_STATE_KEY` of at least 32 bytes. Add `${MCP_PUBLIC_URL}/oauth/callback` as an exact redirect URI to the configured public OIDC client. Keep the state key in the approved secret store and stable across restarts; rotating it invalidates in-flight browser authorization requests. Also set `VAULT_ADDRESS`, `VAULT_ADMIN_TOKEN`, and `VAULT_RUNTIME_TOKEN`. The Vault tokens must be distinct; the runtime token should only issue and revoke approved database leases. The app loads `.env` for local runs, and SQLx reads `DATABASE_URL` from it for the migration targets below. `.env` is ignored by Git; never commit resolved credentials.

For local development only, the gateway permits plain HTTP OIDC/Vault URLs on loopback addresses such as `http://127.0.0.1:8200`. Remote OIDC and Vault endpoints must use HTTPS. Configure the OIDC client with this exact development redirect URI:

```text
http://127.0.0.1:15173/auth/callback
```

#### 3. Create/update the control-plane schema

From the repository root:

```sh
make migrate-up
make migrate-status
```

The gateway also applies embedded migrations at startup. The explicit migration step lets you check connectivity and schema state before starting the processes.

#### 4. Start the gateway and admin UI

In terminal 1, from the repository root:

```sh
make run
```

In terminal 2:

```sh
make admin-dev
```

Open <http://127.0.0.1:15173>. Check the gateway's unauthenticated health endpoints; both return HTTP `204` when healthy (readiness checks the control database only):

```sh
curl -i http://127.0.0.1:8080/health
curl -i http://127.0.0.1:8080/readyz
```

#### 5. Sign in, register a database, and grant access

1. Sign in through the configured OIDC provider using the subject listed in `OIDC_BOOTSTRAP_ADMIN_SUBJECTS`. Its first authenticated request creates the principal and bootstraps it as a platform administrator.
2. In **Databases**, register a reachable PostgreSQL or MySQL target. The gateway validates the connection and sends the supplied database administrator credential to Vault; it does not retain that password in its control database.
3. Each user must sign in at least once so the gateway can create their principal. In **Access**, select that person and database, then grant `read` or `write`. Access is denied by default.

#### 6. Connect an MCP client

For remote MCP clients, use the HTTPS endpoint and let the client discover OAuth from the gateway. The **Conectar MCP** page in the admin UI provides copyable OpenCode V2 and Claude Code setup. OpenCode uses `mcp.servers` with `type: "remote"` and `url`; OAuth is enabled by default. Claude Code can be added with `claude mcp add --transport http <name> <MCP_PUBLIC_URL>/mcp`. When [workspaces](#several-environments-workspaces) are configured, the page names each server after its workspace. Both clients open a browser to sign in; no bearer token belongs in their shared config. The gateway's OAuth access tokens represent the signed-in principal and do not change per-user/per-database grants.

The SQL MCP endpoint is:

```text
${MCP_PUBLIC_URL}/mcp
```

NATS and Valkey are independent services in [`nats-mcp`](../nats-mcp) and [`valkey-mcp`](../valkey-mcp). Each has its own process, control database, admin UI, endpoint, and release lifecycle; configure them from their respective repositories. The SQL gateway does not proxy or manage those MCPs.

For the local stack, `MCP_PUBLIC_URL` defaults to `http://127.0.0.1:8080`; deployments must use their externally reachable HTTPS origin.

Clients should use the OAuth flow above. Manually supplied OIDC access tokens are retained only as a compatibility path; they must have the configured audience and must not be saved in shared config. POST requests must accept both `application/json` and `text/event-stream`. Initialize first, retain the returned `MCP-Session-Id`, send `notifications/initialized`, then include the session ID and negotiated `MCP-Protocol-Version` on subsequent requests. Sessions expire after 30 minutes; HTTP 404 requires fresh initialization. DELETE closes a session. Every request still authenticates and every tool call rechecks grants.

GET accepts `text/event-stream` and opens a finite idle stream; the current tools do not emit unsolicited notifications. POST returns JSON by default, or finite SSE when `text/event-stream` is listed first. Streams have no replay IDs and requests are never automatically retried: a lost SQL response does not prove a write failed. Browser Origins must exactly match the gateway public origin. Reverse proxies must disable SSE buffering and allow streaming responses. Session state is shared in PostgreSQL, with at most 16 live sessions per principal and one 60-second GET stream lease per session.

JSON media types are case-insensitive and may include parameters (for example, `Application/JSON; charset=utf-8`). Clients must send the negotiated protocol-version header; for backwards compatibility, when it is absent the server uses the version stored in the authenticated session. Empty, malformed, duplicate, unsupported or session-mismatched version headers return HTTP 400 on POST, GET and DELETE.

The alternative stdio mode signs in through the browser and refreshes automatically through the gateway OAuth broker. Every command takes an optional workspace (a saved name or a gateway URL) and opens a picker when you leave it out:

```sh
sql-mcp-gateway login                 # first time: paste a gateway URL; later: pick a workspace
sql-mcp-gateway login prod            # sign in to a saved workspace by name
sql-mcp-gateway workspaces            # list workspaces and whether you are signed in
sql-mcp-gateway stdio prod            # the command an MCP client runs
sql-mcp-gateway logout                # sign out; `remove` also forgets the workspace
```

Login stores the access and rotating refresh tokens in the operating system credential store, scoped to that gateway URL, and saves the workspace name and URL in `workspaces.json` under your user configuration directory (no secrets; `SQL_MCP_CONFIG_DIR` overrides the location). Logout revokes the refresh-token family when the gateway is reachable and removes the local credentials.

The stdio process is a **remote client**: it needs only a workspace and the logged-in user's keyring, not control-database, OIDC, Vault, or state-key configuration. Configure the MCP host command as `sql-mcp-gateway` with arguments `["stdio", "prod"]` (or a gateway URL instead of a name). Without an argument it uses `MCP_PUBLIC_URL` when set (including from local `.env`), otherwise the only saved workspace; with several saved workspaces it fails and lists their names instead of guessing. It never prompts. The earlier `--stdio`, `--login` and `--logout <gateway-url>` flags keep working. The gateway handles grants, leases and audit centrally. At EOF the bridge attempts DELETE to release its session. An expired session is reported to the host for reinitialization; tool calls are not replayed.

The credential store must be available and unlocked; the process fails closed rather than writing tokens to a plaintext file. `SQL_MCP_ACCESS_TOKEN` remains temporarily supported for existing installations, but is deprecated and does not refresh automatically. Login/refresh remain keyring-backed; operator secrets must not be copied to MCP host configuration.

## Several environments (workspaces)

Run one gateway per environment; each deployment is an isolated **workspace** with its own control database, identity configuration, Vault, grants and audit trail. To move between them without hunting for URLs, give every deployment the same list and tell each one which entry it is:

```sh
WORKSPACE_SLUG=prod
WORKSPACES='[
  {"slug":"dev","name":"Desenvolvimento","environment":"development","url":"https://sql-dev.example.com"},
  {"slug":"staging","name":"Homologação","environment":"staging","url":"https://sql-staging.example.com"},
  {"slug":"prod","name":"Produção","environment":"production","url":"https://sql.example.com"}
]'
```

`slug` is lowercase letters, digits and hyphens; `name` and `environment` (`production`, `staging` or `development`) are optional; `url` is that gateway's public HTTPS origin. Both variables are optional but must be set together, and the entry selected by `WORKSPACE_SLUG` must use this deployment's `MCP_PUBLIC_URL`, otherwise the gateway refuses to start.

- **Admin UI:** the sidebar shows the current workspace and switches to the same page of another one; production is marked in the frame. Each workspace still runs its own sign-in.
- **MCP clients:** **Conectar MCP** names the server `sql-mcp-gateway-<slug>` and can generate the configuration for every workspace at once, so the environments do not overwrite each other in a client.
- **Command line:** `sql-mcp-gateway login` against any one gateway offers to add the others after showing their addresses; afterwards use names (`login staging`, `stdio prod`) or the picker.

The list is public to anyone who can reach the gateway and grants nothing by itself. See [ADR 0005](docs/adr/0005-federated-workspaces.md).

## Deploy

Deployment is environment-specific: the examples do not create the control database, Vault, target databases, identity provider, or (for AWS) the shared VPC/network. Follow the ordered instructions and validate the plan in your own environment:

1. [Deployment guide index](docs/deployment/README.md) — choose a target and prepare shared dependencies.
2. Follow the target guide for build/package, secret references, routing, and health checks.
3. Review the [production readiness checklist](docs/production-readiness.md) before enabling users.

The OpenTofu examples are starting points, not validated production modules. Their operator-editable `locals.auto.tf` files are copied from ignored example templates. Review IAM, networking, database connection budgets, costs, and the complete plan before you choose to apply anything. This repository has not deployed these examples to an AWS account.

## Usage metrics

The overview includes audit-backed query counts, read/write volume, success rate,
failures, access denials, successful-operation average/p95 latency, time-series
activity, and database/error rankings. Choose the last 24 hours, 7 days, or 30 days;
the interval table provides exact values behind the chart.

`GET /v1/metrics?period=24h` uses the existing bearer authentication and returns
workspace metrics for administrators or personal metrics for members. Scope and
identity cannot be selected by query parameters. Admin-only inventory counts show
the current configuration separately: banks, members, effective grants, active
connections without TLS, and members without an active-bank grant.

Only recorded read/write query attempts count (including read-based schema
inspection), not all MCP/HTTP messages. Audit capture is best-effort. Timings
include gateway/Vault/lease cleanup, not just SQL execution; empty periods never
claim a 100% success rate. See [metric definitions and API boundaries](docs/design/usage-metrics.md).

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
- `docs/architecture/streaming-and-cache-mcps.md`: ownership and separation of the NATS and Valkey MCP services.
- `docs/deployment/`: deployment procedures and OpenTofu examples.

## Tests and quality checks

The regular local checks are:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd apps/admin && pnpm check && pnpm test && pnpm lint && pnpm build
```

`make test-integration` is separate and starts an isolated Docker Compose stack for Vault, PostgreSQL, MySQL, and Keycloak. It requires Python 3 (stdlib only) in addition to Docker, OpenSSL and Cargo. It is not required to run the app locally; read the script and its cleanup behavior before opting in.

To opt in, run `make test-integration` from the repository root. The script generates test-only credentials and local TLS files under `/tmp/opencode`, binds service ports to loopback, uses fixed image versions from `compose.integration.yaml`, checks real database/OIDC behavior, and removes its uniquely named containers, test volumes, and temporary credentials when it finishes. It does not use `.env` or connect to external databases.

Each run performs two passes. Besides real dynamic database leases, it runs the actual gateway binary against Keycloak discovery/JWKS, browser authorization-code/PKCE, signed-token/userinfo validation, consent, code reuse, token rotation/replay/revocation (including concurrent refresh), PostgreSQL grants, Origin checks, session isolation/expiry/admission/termination, POST/GET SSE, and a real stdio subprocess with no operator configuration. The temporary Keycloak realm's wildcard callback and loopback cookie policy are test-only and must not be copied to a deployment.

The same isolated suite checks metrics against real PostgreSQL: more than 100
records, exact period boundaries, zero-filled series, successful-only p95/average,
inventory, member isolation, revoked-target visibility, authenticated routes,
invalid parameters, and role downgrade. The separate UI fixture preview is documented
in `apps/admin/tests/visual/README.md` and never proves real traffic or authentication.
