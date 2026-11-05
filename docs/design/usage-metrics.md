# Usage metrics

## Scope and rationale

Requested after the operator experience redesign: more useful product metrics.
The overview owns read-only usage monitoring; catalog, permissions, and audit
retain their separate operational responsibilities. No additional dependencies,
telemetry agents, database migrations, or external services are introduced.

Actors: administrators inspect workspace activity and configuration exposure;
members inspect only their own activity. A member cannot select another principal
or workspace scope. Current member grants gate names/IDs in the database ranking;
revoked or disabled targets do not become discoverable through historical metrics.

## Contract

`GET /v1/metrics?period=24h|7d|30d` uses the existing identity middleware. The
default period is `24h`. Invalid/unknown parameters use the repository's existing
400 `INVALID_REQUEST` convention. Unauthenticated/forbidden outcomes remain
401/403. Responses are private, `Cache-Control: no-store`.

The backend aggregates recorded `database.read_query` and `database.write_query`
events in `[from, to)`. This includes schema inspection routed through read_query,
but excludes registration, grant changes, lease-revocation events, and transport
messages. These are audited query attempts, not every HTTP request. Audit recording
is currently best-effort; this is not a lossless billing/SLA counter.

- Fixed rolling windows: 24 hourly buckets, or 7/30 daily buckets anchored at
  `from`, including zero buckets. UTC timestamps and bucket length are returned.
- Total, successful, failed, denied, read/write attempts, distinct active people,
  and distinct known database IDs. Success rate is success / all audited attempts;
  no activity is distinct from 100% success.
- Average and continuous p95 duration use successful entries with recorded timing
  only; sample count is returned. Timings include gateway/Vault/lease cleanup, not
  just database execution. Missing timing/rates are omitted, never encoded as null.
- Top five currently identifiable databases by attempt count, and top five
  error-code/outcome groups. Ties have stable ordering. No SQL, fingerprints,
  credentials, database users, or other people's identity details are returned.
- Admin-only inventory counts are a current snapshot, not windowed usage:
  active/paused databases, active databases without TLS, enabled members,
  effective grants, and enabled members without a grant to an active database.

All aggregates share a read-only repeatable-read transaction and fixed bounds.
Existing time/principal indexes support the bounded 30-day maximum; local statement
timeouts bound each query. No automatic refresh loop or optimistic metric values.
No production performance/capacity claim is made from local integration tests.

## Implementation and verification slices

1. Metrics module: typed contract, SQL aggregates, authenticated route, and real
   PostgreSQL/router tests through the isolated integration harness. Cover >100
   events, exact boundaries, empty periods, p95 denominator, and scope isolation.
2. Named frontend data source with runtime validation; present usage separately
   from inventory, preserving loading, no-data, malformed/error, and retry states.
3. Responsive charts/rankings with text equivalents, explicit UTC interval labels,
   period controls and update time. Use only the isolated fixture server for visual
   review; fixture activity never proves production monitoring.

Alternatives rejected: calculating totals from the last 100 audit rows (truncated),
request-time fake graphs (misleading), exposing workspace statistics to members
(privacy), or adding a monitoring platform just for these existing audit counters
(unnecessary scope). Revisit materialized rollups/telemetry when measured event
volume makes bounded on-demand queries insufficient or lossless metrics are needed.

## Verified delivery (2026-11-05)

Implemented in `crates/gateway/src/metrics.rs` and `apps/admin/src/lib/metrics/`.
The overview now obtains actual inventory counts from the metrics endpoint instead
of counting the truncated principal list. Onboarding remains for empty workspaces;
existing workspaces prioritize operational activity.

- `cargo fmt --all -- --check`, `make lint test build`: passed. 35 Rust unit tests
  passed; the live database test remains explicitly ignored in the unit invocation.
- Final frontend `pnpm check`, `pnpm lint`, `pnpm test`, `pnpm build`: passed;
  20 frontend tests, zero Svelte errors/warnings. Tests reject malformed counts,
  wrong periods/scopes, partial time series, nonfinite timings, and privacy leaks.
- `make test-integration`: passed both iterations after the fixture correction
  below. The real migration integration now invokes `verify_metrics_snapshot` on
  its disposable PostgreSQL database. It verifies 114 personal attempts from more
  than 100 rows, p95 105.5ms, successful-only average, exact `[from,to)` bounds,
  24/7/30 zero-filled buckets, inventory, peer isolation, revoked-target ranking
  exclusion, and omission of absent latency/inventory. The real OAuth suite tests
  the HTTP route's 401, invalid-query 400/error envelope, no-store response,
  per-member results, admin snapshot, and role downgrade.
- The first OAuth metrics check exposed an incomplete existing test target:
  its write role was absent, so SQL target decoding failed before the intended
  authorization denial could be audited. The isolated fixture now supplies both
  roles; the new check proves a real denied query was recorded. No production SQL
  authorization logic was changed for this correction.
- Firefox fixture sweep: 92 assertions passed, across six routes at 375, 768, and
  1024px. Added checks cover period switching, interval counts, chart geometry,
  member inventory exclusion, zero activity, and malformed-response rejection.
- New captures and the browser report are in
  `artifacts/verification/usage-metrics/`; a downloadable archive is
  `artifacts/verification/usage-metrics.tar.gz`. Transfer these over the existing
  SSH connection with SCP. Screenshots use synthetic examples, not live traffic.

The real integration proves the aggregate/router contracts, not the browser's
authenticated end-to-end journey against a deployed server. No deployment,
production load/capacity test, billing-grade audit guarantee, or infrastructure
telemetry (CPU/memory/connection health) is claimed. Changes remain local in
`feat/operator-experience`; no commit or push was requested for this extension.
