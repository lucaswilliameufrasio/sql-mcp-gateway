# Sign-in boundary

- **Reach:** open `/` in the local UI with a fresh browser profile.
- **Drive:** wait for session initialization and inspect the SSO button, product name, and any visible configuration error. Do not activate SSO unless an authorized local identity-provider journey is explicitly in scope.
- **Proof:** screenshot shows the rendered logged-out SQL Gateway screen. This does not prove OIDC authentication or an administrator session.
- **Preconditions:** the UI dev server; `/v1/auth/config` is served only when the gateway is running.
- **Gotcha:** a missing local backend may show an identity configuration error; distinguish this setup condition from a rendering defect.
