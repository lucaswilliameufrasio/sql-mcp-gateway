# Usage metrics

- **Reach:** `/`, after authentication. Admins see workspace activity and current
  inventory; members see only their own recorded query activity, without inventory.
- **Drive:** select 24 horas, 7 dias, and 30 dias. Confirm bounds and bucket counts
  change together. Refresh explicitly. Expand “Ver valores por intervalo” for the
  chart's text equivalent. Check failure/no-data/malformed states separately.
- **Proof:** real `/v1/metrics` aggregation is exercised in `make test-integration`:
  inclusive start/exclusive end, >100 records, zero buckets, successful-only
  average/p95, personal isolation, current-grant ranking visibility, current
  inventory, authentication, invalid parameters, and role downgrade.
- **UI proof:** explicit fixture preview regression tests period controls, chart
  geometry, empty rate/latency, lack of member inventory, and malformed response
  rejection. Fixtures do not establish real traffic or persisted metric values.
- **Semantics:** only audit events `database.read_query` and `database.write_query`
  count, including read-based schema inspection. Timings include gateway/Vault and
  lease cleanup. Audit capture is best-effort, not a billing/SLA guarantee.
- **Privacy:** no caller-supplied identity/scope. Historical totals for members may
  include now-revoked targets, but their names/IDs must not appear in rankings.
- **Remote review:** captures are files on the SSH host; transfer them via SCP or
  use an explicitly requested loopback SSH tunnel, never an unauthenticated public
  preview service.
