# Access and audit

- **Reach:** sign in locally as an administrator; visit `/access`, then `/audit`.
- **Drive:** in “Pessoas e acessos”, choose a disposable test principal and target. Check read/write explanations, save the least privilege required, and inspect the grant list. Revocation requires confirmation; cancel/Escape does not send DELETE. In audit, filter result/search within the latest 100 events and clear filters when no matches remain.
- **Proof:** the grant's principal, database, and access level are visible; the corresponding event appears in audit. Confirm persisted state through the documented local API/test setup.
- **Preconditions:** isolated local gateway, seeded test principal/target, and administrator identity.
- **Gotcha:** do not use real identities or shared databases. Without the local stack, leave this journey unverified.
- **Errors:** a failed grant load must not show a stale prior person's permissions or enable saving. A failed mutation retains its error and permits retry; it must not claim success.
