# MCP client onboarding

- **Reach:** any authenticated identity can open `/connect`.
- **Drive:** choose OpenCode, Claude Code, then another client. Confirm each choice
  changes the instructions and copyable content. Verify clipboard feedback, OAuth
  discovery metadata for generic clients, and the expanded stdio alternative.
- **Proof:** copied configuration contains the gateway endpoint, never bearer
  tokens or database credentials. Configuration copying does not prove MCP login.
- **Member journey:** `/` shows actual individual grants and guidance when no grants
  exist. Administrative links are absent; direct administrative routes explain the
  restriction, without attempting administrative API requests.
- **Preconditions:** local authenticated test session for real behavior. The visual
  fixture server can establish only rendering, selection, and copy feedback.
- **Gotcha:** never use the visual fixture identity to claim authorization. Only a
  real client login against the isolated local gateway can establish OAuth success.
