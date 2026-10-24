# Deploy on a VPS

This procedure runs the Rust gateway and SvelteKit admin UI as separate systemd services. Both listen on loopback; Caddy or Nginx presents one private TLS origin. It covers a new proxy installation and integration into an existing Caddy/Nginx host.

## Before you start

Have these ready:

- A Linux VPS with systemd, private/VPN routes to Vault and every target database, and outbound HTTPS to the OIDC discovery/JWKS endpoints.
- A dedicated PostgreSQL database for gateway control-plane metadata. The gateway applies its embedded migrations at startup; its database identity needs migration rights in this database only.
- An OIDC client/provider. Register the final admin UI origin with the callback `/auth/callback` (for example `https://sql-gateway.example.internal/auth/callback`). Bootstrap admin access is based on the exact OIDC `sub` value, not email.
- Vault with the PostgreSQL and/or MySQL database secrets engines configured, separate administration/runtime tokens, and a secure secret-delivery mechanism on the VPS.
- DNS and a TLS certificate trusted by the approved browser/MCP clients. Keep the host and target databases private; do not open database ports to the internet.
- Rust/Cargo and Node.js/pnpm on a build host, or release artifacts built for the VPS's OS, CPU architecture, and libc family.

## Deploy the application

### Container deployment with Docker Compose

For a host where Docker Compose is already operated, [`deploy/docker/compose.production.yaml`](../../deploy/docker/compose.production.yaml) runs the production gateway and admin images. It is separate from `compose.dev.yaml` and does not start a database, OIDC provider, Vault, or reverse proxy; those remain separately managed production dependencies. The gateway and admin ports bind only to host loopback for the existing Caddy/Nginx proxy configuration below.

Create root-owned runtime env files at `/etc/sql-mcp-gateway/gateway.env` and `/etc/sql-mcp-gateway/admin.env` using the approved secret-delivery workflow. The gateway file needs the variables listed under [Deliver runtime configuration](#3-deliver-runtime-configuration); the admin file needs `ORIGIN` set to the externally visible HTTPS origin, for example `https://sql-gateway.example.internal`. Set each file to mode `0640`, owned by `root` and readable only by the account that operates Docker. Do not put secret values in the repository or a Compose `.env` file.

From a checkout/release directory on the host, build and start the services:

```sh
docker compose -f deploy/docker/compose.production.yaml config --quiet
docker compose -f deploy/docker/compose.production.yaml build --pull
docker compose -f deploy/docker/compose.production.yaml up --detach --wait
```

Use immutable release tags for `GATEWAY_IMAGE` and `ADMIN_IMAGE` when building a release. The Compose file exposes only `127.0.0.1:8080` and `127.0.0.1:3000`; keep TLS termination and external routing in the existing host proxy. Follow the health checks, onboarding, upgrade, and rollback guidance below. `docker compose down` stops the application but preserves images; do not add volume-removal flags to routine deployment commands.

### 1. Build release artifacts

From the repository root on the build host:

```sh
cargo build --locked --release -p sql-mcp-gateway
cd apps/admin
pnpm install --frozen-lockfile
pnpm build
```

Transfer the gateway binary at `target/release/sql-mcp-gateway` and the admin UI's `build/`, `node_modules/`, and `package.json` to the VPS. Preserve the layout below. Build on a compatible Linux host/container; do not copy a binary built for another OS or architecture.

### 2. Create service users and install the artifacts

Run once on the VPS. If either service account already exists, keep and verify that account rather than creating a duplicate:

```sh
sudo useradd --system --user-group --home-dir /nonexistent --shell /usr/sbin/nologin sql-mcp-gateway
sudo useradd --system --user-group --home-dir /nonexistent --shell /usr/sbin/nologin sql-mcp-admin
sudo install -d -o root -g root -m 0755 /opt/sql-mcp-gateway/apps/admin
```

From the checkout/release directory on the VPS, install the matching artifacts:

```sh
sudo install -m 0755 target/release/sql-mcp-gateway /opt/sql-mcp-gateway/sql-mcp-gateway
sudo cp -a apps/admin/build apps/admin/node_modules apps/admin/package.json /opt/sql-mcp-gateway/apps/admin/
```

The systemd templates assume these paths and service account names. The admin process runs `node build` from `/opt/sql-mcp-gateway/apps/admin`.

### 3. Deliver runtime configuration

Create `/etc/sql-mcp-gateway/gateway.env` through your approved secret manager or root-controlled provisioning workflow. Set owner `root:sql-mcp-gateway` and mode `0640`. Do not put secret values in command arguments, the repository, or deployment notes.

Set at least `CONTROL_DATABASE_URL`, `OIDC_ISSUER_URL`, `OIDC_CLIENT_ID`, `OIDC_ACCESS_TOKEN_AUDIENCE`, `OIDC_BOOTSTRAP_ADMIN_SUBJECTS`, `VAULT_ADDRESS`, `VAULT_ADMIN_TOKEN`, and `VAULT_RUNTIME_TOKEN`. The Vault tokens must be distinct. `BIND_ADDRESS=127.0.0.1:8080` is already supplied by the gateway unit. The control URL must target the dedicated control-plane database, not a registered target.

Set `ORIGIN` in `deploy/vps/sql-mcp-admin.service` to the exact external HTTPS origin before installing the unit. This is used by the SvelteKit Node adapter and must match the OIDC redirect URI origin. Keep the original `HOST=127.0.0.1` and `PORT=3000` values.

### 4. Install and start the systemd services

From the repository/release directory containing `deploy/vps/`:

```sh
sudo install -m 0644 deploy/vps/sql-mcp-gateway.service /etc/systemd/system/
sudo install -m 0644 deploy/vps/sql-mcp-admin.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now sql-mcp-gateway sql-mcp-admin
```

The gateway applies its embedded SQLx migrations at startup. Its control-plane connection pool allows up to 20 connections per process. Budget those connections and test schema changes against an isolated database before upgrading production.

Check service state and logs without printing the environment file:

```sh
sudo systemctl status sql-mcp-gateway sql-mcp-admin
sudo journalctl -u sql-mcp-gateway -u sql-mcp-admin --since today
```

## Put the services behind a reverse proxy

Do not publish ports `8080` or `3000`. Expose only TLS to approved VPN/private-network peers. Proxy `/mcp`, `/v1/*`, `/health`, and `/readyz` to the gateway at `127.0.0.1:8080`; proxy all other UI routes (including `/auth/callback`) to the admin service at `127.0.0.1:3000`. Preserve the `Authorization` header on `/mcp` and `/v1/*`.

### New VPS without Caddy or Nginx

Install Caddy from the package source supported by your Linux distribution. Replace the sample hostname in [`deploy/vps/Caddyfile.example`](../../deploy/vps/Caddyfile.example), then install and enable it:

```sh
sudo install -m 0644 deploy/vps/Caddyfile.example /etc/caddy/Caddyfile
sudo caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile
sudo systemctl enable --now caddy
```

The example uses Caddy's internal CA for a private hostname; distribute and trust that CA on approved clients before connecting. If you use a publicly trusted certificate, select a challenge method compatible with the private origin and do not open unnecessary firewall ports.

### Existing Caddy site

Merge the matchers/handlers from [`deploy/vps/Caddyfile.example`](../../deploy/vps/Caddyfile.example) into the existing site block, then validate and reload Caddy using your normal change process. For the default Caddyfile path:

```sh
sudo caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile
sudo systemctl reload caddy
```

Preserve the existing site's other routes, TLS policy, and snippets; do not add a duplicate site block for the same hostname.

### Existing Nginx server

Merge [`deploy/vps/nginx-server.example.conf`](../../deploy/vps/nginx-server.example.conf) into the existing TLS virtual host, then validate and reload Nginx. For the standard systemd service name:

```sh
sudo nginx -t
sudo systemctl reload nginx
```

Keep the existing certificates and unrelated routes. Replace the sample health-check CIDR with the actual monitoring/VPN range; the example restricts `/health` and `/readyz` but they are unauthenticated. `/v1/auth/config` is also unauthenticated and should be reachable only through the trusted origin.

## Verify and onboard

From an approved private/VPN client network, verify the proxy and service state:

```sh
curl -i https://sql-gateway.example.internal/health
curl -i https://sql-gateway.example.internal/readyz
```

Both return `204` when successful. `/health` confirms the process responds; `/readyz` additionally checks the control-plane database only. Neither proves Vault or target-database health.

Then sign in through the admin UI with the configured bootstrap subject, register a target database (the supplied database admin credential is sent to Vault), have each user sign in once, and grant only the required per-user/per-database access. Test an authenticated MCP `POST /mcp` request using an OIDC access token whose audience matches the gateway setting. Complete the shared [production readiness checklist](../production-readiness.md) before enabling real users.

## Upgrade and rollback

Build an immutable release and keep the previous binary and admin build. Back up the control-plane database and review embedded migration changes before switching files and restarting services. Since migrations run at gateway startup, an upgraded schema may not work with the previous binary; test rollback after each schema change in an isolated environment. A simple binary rollback is not a database rollback plan.
