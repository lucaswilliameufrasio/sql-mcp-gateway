---
name: verify-sql-mcp-gateway
description: Verify the SQL MCP Gateway admin web UI and its documented operator journeys.
---

# Verify SQL MCP Gateway

## Launch

The primary surface is the SvelteKit admin UI in `apps/admin`. From that directory:

```sh
pnpm dev --port 5178
```

The UI proxies `/v1` and `/mcp` to `127.0.0.1:8080`. A usable authenticated admin journey requires the local SQL gateway, its development identity provider, and isolated test data; follow the repository README and Makefile for the supported local stack. Never source `.env` or `.env.dev` into a shell for UI-only verification. Without the backend, only the logged-out state and frontend presentation are verifiable. Stop only the dev-server process started for this run (Ctrl-C in its terminal).

## Preflight

1. From `apps/admin`, run `pnpm check`, then `pnpm build`; record the outcomes.
2. Confirm the intended instance owns the selected port: `ss -ltnp 'sport = :5178'`. If occupied, choose another free loopback port and use it consistently. Do not kill an unknown process.
3. Start the dev server and wait for Vite's ready message; confirm `http://127.0.0.1:5178/` and `/favicon.svg` return HTTP 200 before opening the app in Firefox.
4. Backend-dependent journeys require the documented local stack to be healthy. Do not infer health from the UI server's ready message.

## Drive and evidence

Use Firefox at the local URL. Navigate and interact by accessible link/button names and labels; do not use coordinates as selectors. Verify the visible result after each action. Capture screenshots and console/network errors in `artifacts/verification/` (create the directory only for this run; preserve evidence after stopping the server). Never click SSO during routine UI verification: it leaves the local UI for the identity provider and is not a local-only action.

`features/README.md` maps the supported journeys. A logged-out screen is evidence only for the sign-in boundary, not proof of database administration, grants, or audit behavior. For authenticated journeys, verify the visible state and the API/persisted result using the repository's local test setup; never use shared or production data.

## Isolation, cleanup, and failures

Use an unused loopback port. Do not modify environment files, use real credentials, kill processes by name, or clean up resources not created by this run. Preserve screenshots/log excerpts after server teardown. Report missing backend/OIDC setup separately from an application defect; keep sign-in, administrator, and data-dependent behavior unverified if no local identity/test fixture is available. Update `features/` whenever routes, labels, or journey behavior changes.
