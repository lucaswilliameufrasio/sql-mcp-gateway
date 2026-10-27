# Database catalog

- **Reach:** sign in locally as an administrator, then open `/databases`.
- **Drive:** use the labeled target form to add a disposable local database target; confirm it appears in the catalog, then exercise enable/disable only on that fixture.
- **Proof:** visible catalog row and resulting enabled state; use the documented local integration test or API to confirm persisted state.
- **Preconditions:** isolated local gateway, test database/Vault configuration, and local administrator identity as documented by the repository.
- **Gotcha:** never enter production credentials; validation and Vault behavior cannot be proven with the UI alone.
