# Isolated UI preview

Run from `apps/admin`:

```sh
pnpm exec vite dev --config vite.visual.config.ts --port 5179
```

This explicit development-only config replaces the session module with synthetic
identities and read-only examples. It has no API proxy and performs no OIDC login.
The normal dev/build config does not import this fixture. Do not deploy it.
Writes intentionally fail instead of pretending to persist changes.

Open any route with `?role=admin`, `?role=member`, or `?role=logged-out`.
Use `&scenario=empty` or `&scenario=error` for collection and failure states.
Reload when changing fixture identity/scenario. This is presentation evidence,
not an integration or authorization test.

For deterministic narrow viewports, open `/__visual` on the
fixture server. It renders the real routes in a 375/768/1024px iframe, reports
page-level horizontal overflow, and counts runtime errors observed after load.
This is not a full console/network capture or an automated accessibility audit.
The “Verificar layouts e interações” action runs a browser regression sweep over
all six routes at 375, 768, and 1024px, then checks filtering, confirmation/cancel,
TLS/engine modes, client selection, member guidance, and empty/error distinctions.
Its visible report includes each visited viewport and the assertion total.

Check desktop and narrow viewports:

- `/`: task sequence for admins, grants and connection guidance for members.
- `/`: audit-backed usage cards, 24h/7d/30d controls, volume chart with interval
  table, error/database rankings, and admin-only current inventory. `malformed`
  scenario verifies runtime rejection, not a zero-valued success fallback.
- `/databases`: search by name/host, filter paused banks, clear unmatched filters.
- `/databases/new`: switch fields/URL, change engine/port, disable TLS and check
  warning/CA exclusion; required fields block invalid submission.
- `/access`: select a person, inspect read/write explanation, open and cancel
  revocation, verify error retention when a fixture write is attempted.
- `/audit`: outcome/search filters, no results, refresh and failure recovery.
- `/connect`: choose all three clients; copy config, expand stdio; OAuth metadata
  appears for generic clients. No tokens are included in copied content.
- Keyboard: visible focus, skip link, dialog traps focus, Escape cancels and
  restores focus to its trigger; sign-out remains available at narrow widths.

Use the regular dev config for the logged-out integration boundary and the
repository's real local stack for any claims about successful writes or login.
