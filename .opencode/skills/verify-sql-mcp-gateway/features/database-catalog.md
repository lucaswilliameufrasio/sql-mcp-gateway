# Database catalog

- **Reach:** sign in locally as an administrator, then open `/databases`.
- **Drive:** click “Cadastrar banco” to open `/databases/new`. Exercise explicit TLS and fields/URL modes; register a disposable local target, then follow “Ver bancos”. Search by name/host, filter active/paused, and clear unmatched filters. Pause requires confirmation; Escape/cancel must leave the target unchanged.
- **Proof:** visible catalog row and resulting enabled state; use the documented local integration test or API to confirm persisted state.
- **Preconditions:** isolated local gateway, test database/Vault configuration, and local administrator identity as documented by the repository.
- **Gotcha:** never enter production credentials; validation and Vault behavior cannot be proven with the UI alone.
- **Member boundary:** direct navigation to either administrative route shows a restriction message; only the API can enforce actual authorization.
