# VPS deployment

This guide runs the gateway and administration UI as separate systemd services.
The gateway listens on loopback and the reverse proxy presents one TLS origin.
It covers a new Caddy site and integration with an existing Caddy or Nginx
server. Replace example hostnames, paths, and VPN ranges before use.

## Prerequisites

- A supported Linux VPS with systemd, Rust/Cargo for building the gateway, and
  Node.js/pnpm for building the admin UI (or install release artifacts built
  in CI for the VPS architecture and OS).
- Private network/VPN routes from the VPS to Vault and every registered target,
  plus outbound HTTPS access to the OIDC discovery/JWKS endpoints.
- A dedicated OS user for each service and a secret-delivery process. Do not
  commit an environment file or put secret values in shell arguments.
- A DNS name and TLS certificate trusted by the MCP clients and administrators.

## Build and install

Build the artifacts from the repository root, preferably in a CI job that uses
the same Linux architecture and libc family as the VPS:

```sh
cargo build --locked --release -p sql-mcp-gateway
cd apps/admin
pnpm install --frozen-lockfile
pnpm build
```

Install the gateway binary at `/opt/sql-mcp-gateway/sql-mcp-gateway` and the
contents required by the SvelteKit Node adapter under
`/opt/sql-mcp-gateway/apps/admin/` (including the generated `build/` directory
and production Node dependencies). The service templates in `deploy/vps/`
assume these paths and users named `sql-mcp-gateway` and `sql-mcp-admin`.
Edit the admin unit's `ORIGIN` to the exact external HTTPS origin used by the
browser and registered with OIDC.

Create `/etc/sql-mcp-gateway/gateway.env` with mode `0640`, owned by
`root:sql-mcp-gateway`, and supply the required settings from your secret
manager. At minimum configure `CONTROL_DATABASE_URL`, `OIDC_ISSUER_URL`,
`OIDC_CLIENT_ID`, `OIDC_ACCESS_TOKEN_AUDIENCE`,
`OIDC_BOOTSTRAP_ADMIN_SUBJECTS`, `VAULT_ADDRESS`, `VAULT_ADMIN_TOKEN`, and
`VAULT_RUNTIME_TOKEN`. The two Vault tokens must be distinct. Keep the control
database dedicated to gateway metadata; do not put target database credentials
there. `BIND_ADDRESS` is set to `127.0.0.1:8080` by the supplied unit.

The gateway currently applies its embedded SQLx migrations at process startup.
Its control-plane PostgreSQL pool allows up to 20 connections per process.
Give its DB identity migration rights only in the dedicated control database,
and include this startup migration behavior in restart/upgrade planning.

Install and enable the service units:

```sh
sudo install -m 0644 deploy/vps/sql-mcp-gateway.service /etc/systemd/system/
sudo install -m 0644 deploy/vps/sql-mcp-admin.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now sql-mcp-gateway sql-mcp-admin
```

Check service state and logs without printing environment values:

```sh
sudo systemctl status sql-mcp-gateway sql-mcp-admin
sudo journalctl -u sql-mcp-gateway -u sql-mcp-admin
```

## VPS without an existing reverse proxy: install Caddy

For a new host with no Caddy/Nginx, install and enable Caddy using your Linux
distribution's supported package source, then adapt the supplied Caddyfile.
Keep the host firewall and VPN ACL limited to approved peers; the site example
assumes that network boundary already exists.

## Existing Caddy site

Copy or adapt [`deploy/vps/Caddyfile.example`](../../deploy/vps/Caddyfile.example)
into the active Caddy configuration. It routes `/mcp`, `/v1/*`, `/health`, and
`/readyz` to the gateway, and other paths to the admin UI. The example uses
Caddy's internal CA for a private hostname; distribute and trust that CA on
approved clients before connecting. If using a publicly trusted certificate,
choose a challenge method compatible with a VPN-only origin and do not open
unneeded public firewall ports.

When Caddy already owns the hostname, merge the matchers/handlers into the
existing site block and preserve its other routes, TLS policy, and snippets.
Avoid defining duplicate site blocks for the same host.

## Existing Nginx server

Adapt [`deploy/vps/nginx-server.example.conf`](../../deploy/vps/nginx-server.example.conf)
inside the existing TLS virtual host. Keep the TLS certificate and unrelated
locations owned by the existing site. Restrict `/health` and `/readyz` to the
actual monitoring/VPN CIDRs; the example CIDRs are placeholders. Nginx must
preserve the `/v1/...` and `/mcp` paths and forward the `Authorization` header.

For either proxy, expose only TLS to approved VPN peers. Do not publish port
8080 or 3000, and restrict the unauthenticated `/health`, `/readyz`, and
`/v1/auth/config` routes at the network/proxy boundary. `/readyz` checks the
control-plane database only; it does not prove Vault or target database health.

## Upgrade and rollback

Build an immutable release, back up the control-plane database, review the
embedded migration changes, then replace the gateway binary and restart its
service. Keep the prior binary and admin build available for rollback. A
database migration can make an old binary incompatible, so test rollback after
each schema change in an isolated environment. Complete the shared
[production-readiness checklist](../production-readiness.md) before go-live.
