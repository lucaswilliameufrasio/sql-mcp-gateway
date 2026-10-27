# SQL MCP Gateway UI feature map

This map covers representative user-facing routes, not every interaction. Start with [`../SKILL.md`](../SKILL.md). Authenticated journeys need the repository's isolated local stack and test identities; do not substitute production access.

| Feature          | Route               | Primary actor | Proof                                                                                     |
| ---------------- | ------------------- | ------------- | ----------------------------------------------------------------------------------------- |
| Sign-in boundary | `/`                 | Any user      | Product identity, SSO action, and OIDC help/error state are visible; no login is claimed. |
| Database catalog | `/databases`        | Administrator | Create/edit/enable flow completes and the resulting target appears in the catalog.        |
| Access and audit | `/access`, `/audit` | Administrator | Grant level is reflected in the grant list; a resulting event is visible in audit.        |

Update this map when routes, controls, or permissions change.
