# Access and audit

- **Reach:** sign in locally as an administrator; visit `/access`, then `/audit`.
- **Drive:** choose a disposable test principal and target, grant the least privilege required, and inspect the grant list and audit events.
- **Proof:** the grant's principal, database, and access level are visible; the corresponding event appears in audit. Confirm persisted state through the documented local API/test setup.
- **Preconditions:** isolated local gateway, seeded test principal/target, and administrator identity.
- **Gotcha:** do not use real identities or shared databases. Without the local stack, leave this journey unverified.
