# SQL MCP Gateway UI feature map

This map covers representative user-facing routes, not every interaction. Start with [`../SKILL.md`](../SKILL.md). Authenticated journeys need the repository's isolated local stack and test identities; do not substitute production access.

| Feature          | Route               | Primary actor | Proof                                                                                     |
| ---------------- | ------------------- | ------------- | ----------------------------------------------------------------------------------------- |
| Sign-in boundary | `/`                 | Any user      | Product identity, SSO action, and OIDC help/error state are visible; no login is claimed. |
| Database catalog | `/databases`, `/databases/new` | Administrator | Search/filter; register via separate page; confirm pause and verify persisted state. |
| Access and audit | `/access`, `/audit` | Administrator | Grant level is reflected in the grant list; a resulting event is visible in audit.        |
| MCP onboarding | `/connect` | Any authenticated user | Select a client, copy configuration without tokens, and inspect OAuth/stdio guidance. |
| Usage metrics | `/` | Administrator / member | Switch 24h/7d/30d; scope is workspace for admins, personal for members; inspect chart, p95, rankings, and current inventory. |

Update this map when routes, controls, or permissions change.

For UI-only review use `apps/admin/tests/visual/README.md`. Its synthetic read-only
fixtures never establish authenticated backend behavior or successful mutations.
