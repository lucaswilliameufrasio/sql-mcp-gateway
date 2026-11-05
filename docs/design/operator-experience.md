# Operator and MCP user experience

## Scope and direction

The redesign preserves the gateway APIs, OIDC authentication, per-database grants,
and explicit TLS policy (ADR 0001 and ADR 0004). No new dependencies or external
font requests are needed. The interface remains in Portuguese.

Actors and journeys:

- Administrators register a database, grant a person access, and inspect activity.
  The catalog (`/databases`) and registration (`/databases/new`) are separate tasks.
  Registration explains provisioning credentials, TLS, errors, and the next action.
- Members see their actual grants and connect their preferred MCP client.
  `/connect` presents one client's setup at a time, with copy feedback, OAuth
  instructions, and a way to diagnose missing database access.
- Audit is read-only; filters apply to the latest 100 loaded events, not the entire
  historical log. Revocation and database pause require explicit confirmation.

Loading, no data, no matching results, request failure, and success must remain
distinct. Session identity is not proof of database or gateway health. No decorative
health claims, fake activity, or invented telemetry belong in the production UI.

## Visual system

- Canvas `#f5f7fa`, surface `#ffffff`, ink `#172b3a`, secondary `#536574`,
  action blue `#2359c4`, success teal `#136b54`.
- Locally available humanist sans-serif (`Segoe UI`, system fallback); monospace
  only for endpoints, commands, and identifiers. Body 16px, controls 14px,
  headings 28–36px, readable metadata at least 12px.
- Light navigation rail with task groups; left-aligned content, restrained borders,
  generous form spacing, compact data rows. The connection path is the distinctive
  visual device; decorative grids, oversized KPI cards, and all-caps labels are removed.
- Visible keyboard focus, semantic SVG icons, 44px controls, responsive navigation
  with sign-out available on small screens, reduced-motion support.

The first generated design recommendation was a documentation landing page and did
not fit an operational console. The narrower enterprise dashboard recommendation
supports restrained hierarchy, but its marketing hero and orange CTA were also
rejected: the primary surface is a working tool, not a landing page.

## References

- [Carbon empty states](https://carbondesignsystem.com/patterns/empty-states-pattern/):
  explain the next useful action and distinguish empty catalogs from filtered results.
- [Radix Themes](https://www.radix-ui.com/themes/docs/overview/getting-started):
  consistent control hierarchy and neutral surfaces; reference only, not a React dependency.
- Local `ui-ux-pro-max`: enterprise dashboard visual hierarchy and accessibility
  priorities. Existing Svelte 5 runes and authenticated API adapter remain authoritative.

## Verification boundaries

Frontend checks and UI fixture screenshots demonstrate presentation and interaction,
not real authorization or persistence. The isolated integration suite remains the
evidence for the actual gateway, Vault, SQL engines, and OAuth contracts. No shared
environment credentials are used for visual review.

## Delivery evidence (2026-11-05)

Working branch: `feat/operator-experience`, based on `de009bb`. Changes are local;
no commit, push, merge, release, or deployment is claimed.

| Outcome | Implementation | Verification |
| --- | --- | --- |
| Coherent product UI and task navigation | Shared tokens, SVG icons, shell, sign-in diagram, role-specific overview | Firefox desktop screenshots; all six routes at 375/768/1024px |
| Operator catalog and registration flow | `/databases` search/state filters; `/databases/new` form and next-action success screen | Browser checks for filters, confirmation/cancel, engine/port, URL and explicit TLS modes; successful UI registration remains unverified against a real backend |
| Clear permission controls | Person context, read/write explanations, modal revocation, loading/error protection | Browser fixture interactions and real backend permission integration tests; no real grant/revoke was submitted through the redesigned UI |
| MCP user onboarding | One selected client, copy feedback, OAuth discovery, stdio alternative, member grant guidance | Browser selection/copy inspection and frontend configuration tests; real client login from this page was not exercised |
| Failure and empty states differ | Retry actions, scoped no-results messages, explicit member restrictions | Browser regression assertions for missing grants, empty catalog, request failure, and direct member navigation |

Checks run:

- `cargo fmt --all -- --check` and `make lint test build` passed. Rust: 34 unit
  tests passed, with the live integration case intentionally ignored in the unit
  invocation. Frontend: 11 tests passed; Svelte check reported zero errors/warnings.
- After the final overview copy adjustment, frontend `pnpm check`, `pnpm lint`,
  `pnpm test`, and `pnpm build` were repeated successfully.
- `make test-integration` passed both iterations with isolated PostgreSQL, MySQL,
  Vault, and Keycloak, including SQL grants, OAuth, SSE/sessions, remote stdio, and
  refresh/revocation. Its temporary containers/network were removed by the script.
  Backend files were not changed by this redesign.
- The fixture viewport regression passed 73 assertions. The first mobile catalog
  review exposed page overflow caused by the absolutely positioned accessible
  table caption escaping its scrolling container. Positioning `.table-scroll`
  relatively contains it; the sweep now checks all six routes at three widths.
- One sweep timed out while a build was running against the same SvelteKit output;
  the complete sweep was rerun after the build, without concurrent build output
  changes. Keep build and browser sweeps sequential when reproducing.
- Normal production build output was searched for fixture identity markers; none
  were found. The fixture replacement plugin applies only to explicit dev serving.

Local evidence: `artifacts/verification/operator-experience/` contains representative
desktop/mobile screenshots and `browser-regression.txt`. Authenticated screenshots
are synthetic and are not proof of real authentication, authorization, or writes.
The standard dev server's sign-in screen was also inspected without activating SSO.
No full accessibility audit, assistive-technology certification, production rollout,
or changes to the infrastructure examples are included in this delivery.
