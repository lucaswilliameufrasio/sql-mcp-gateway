# SQL MCP Gateway — usage metrics preview

All screenshots contain synthetic fixture data, not production activity.

- `metrics-workspace.png`: administrator overview with query volume, outcomes,
  successful-operation average/p95, trends, rankings, and current inventory.
- `metrics-personal.png`: member overview; only personal activity, no workspace
  inventory or other people's identity details.
- `metrics-mobile.png`: 375px viewport preview.
- `browser-regression.txt`: the 92-assertion Firefox regression result.

Backend evidence is separate: real PostgreSQL aggregate tests and authenticated
Keycloak HTTP checks passed in both iterations of `make test-integration`.
The frontend had 20 passing unit tests, plus successful typecheck, lint, and build.
See `docs/design/usage-metrics.md` for exact definitions and limitations.

To download from an SSH client, replace `SEU_HOST_SSH` with the same connection
alias or user@host used to connect, and run on your own computer:

```sh
scp SEU_HOST_SSH:/home/lucaseufrasio/projects/sql-mcp-gateway/artifacts/verification/usage-metrics.tar.gz .
tar -xzf usage-metrics.tar.gz
```
