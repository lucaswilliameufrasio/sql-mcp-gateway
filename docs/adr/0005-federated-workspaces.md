# ADR 0005: Federated workspaces for several environments

- Status: Accepted
- Date: 2026-11-11
- Scope: operating one gateway per environment (for example development, staging
  and production) from the admin UI, MCP clients and the command line.

## Decision

A **workspace** is one gateway deployment. Workspaces stay fully isolated: each has
its own control database, OIDC configuration, Vault, grants, audit trail and
sessions. A deployment may only _advertise_ its siblings.

Operators give every deployment the same `WORKSPACES` JSON list and a
`WORKSPACE_SLUG` naming its own entry. Both are optional and must be set together.
Startup fails when the slug is not in the list or when that entry's URL is not the
`MCP_PUBLIC_URL` origin, so a deployment cannot claim another environment's
identity by a copy-paste mistake. `GET /v1/auth/config` returns `current_workspace`
and `workspaces`; both are omitted when unconfigured.

- **Admin UI:** shows the current workspace and environment, marks production, and
  links to the same page of a sibling deployment. Switching is a normal navigation
  to the other origin, which runs its own sign-in.
- **MCP clients:** the onboarding page names the server `sql-mcp-gateway-<slug>`,
  so several environments coexist in one client, and can emit all of them at once.
- **Command line:** `login`, `logout`, `workspaces`, `remove` and `stdio` take an
  optional workspace (saved name or gateway URL) and open a picker when it is left
  out. Names and URLs are saved in `workspaces.json` under the user configuration
  directory (`SQL_MCP_CONFIG_DIR` overrides it). Tokens remain in the OS credential
  store, still keyed by gateway URL. The earlier `--stdio/--login/--logout <url>`
  flags keep working.

## Trust boundaries

- The workspace list is served without authentication, like the rest of
  `/v1/auth/config`, because the sign-in screen and the first command-line login
  need it. It discloses sibling names and origins to anyone who can reach the
  gateway; keep gateways on private routes as already required.
- A gateway is not trusted to name other gateways. The command line shows
  advertised siblings with their URLs and saves them only after confirmation,
  accepts only HTTPS origins (HTTP on loopback), keeps the address the person
  typed for the gateway being signed in to, and never moves a saved name to a
  different URL. The admin UI renders only `http(s)` links.
- No token, session or grant crosses workspaces.

## Alternatives and consequences

A central console calling every gateway from one origin was rejected: it needs
CORS on each gateway, production included, and holds tokens for all environments
in one browser origin. Modelling environments inside one gateway was rejected: it
merges network, Vault and audit boundaries that the environments exist to separate.

Consequences: the list is static configuration, so adding an environment means
updating and restarting each deployment; a deployment missing the update simply
does not show the new sibling. `stdio` never prompts because its stdin/stdout carry
MCP messages; with several saved workspaces and no argument it fails with the
available names. Revisit if workspaces need dynamic registration or cross-workspace
reporting.
