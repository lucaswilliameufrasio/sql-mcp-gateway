import type {
  MetricsPeriod,
  MetricsScope,
  MetricsSnapshot,
  MetricsBucket
} from '../../src/lib/metrics/model';

// Deterministic UI/test data, never imported by the normal application config.
export function metricsFixture(
  period: MetricsPeriod = '24h',
  scope: MetricsScope = 'workspace',
  empty = false
): MetricsSnapshot {
  const bucketSeconds = period === '24h' ? 3600 : 86400;
  const bucketCount = period === '24h' ? 24 : period === '7d' ? 7 : 30;
  const to = Date.parse('2026-11-05T12:00:00Z');
  const from = to - bucketCount * bucketSeconds * 1000;
  const wave = [
    17, 11, 6, 4, 2, 5, 12, 27, 46, 73, 94, 67, 59, 81, 102, 86, 64, 40, 28, 31, 52, 36, 22, 14
  ];
  const series: MetricsBucket[] = Array.from({ length: bucketCount }, (_, index) => {
    const total = empty
      ? 0
      : scope === 'personal'
        ? Math.ceil(wave[index % wave.length] / 6)
        : wave[index % wave.length] * (period === '24h' ? 1 : 7);
    const failed = total > 12 ? Math.floor(total / 17) : 0;
    const denied = total > 8 ? Math.floor(total / 23) : 0;
    return {
      start: new Date(from + index * bucketSeconds * 1000).toISOString(),
      total_queries: total,
      successful_queries: total - failed - denied,
      failed_queries: failed,
      denied_queries: denied
    };
  });
  const counts = series.reduce(
    (total, bucket) => ({
      total_queries: total.total_queries + bucket.total_queries,
      successful_queries: total.successful_queries + bucket.successful_queries,
      failed_queries: total.failed_queries + bucket.failed_queries,
      denied_queries: total.denied_queries + bucket.denied_queries
    }),
    { total_queries: 0, successful_queries: 0, failed_queries: 0, denied_queries: 0 }
  );
  const writeQueries = Math.floor(counts.total_queries / 9);
  const topErrors: MetricsSnapshot['top_errors'] = [];
  if (counts.failed_queries > 0) {
    topErrors.push({
      error_code: 'QUERY_TIMEOUT',
      outcome: 'failure',
      count: counts.failed_queries
    });
  }
  if (counts.denied_queries > 0) {
    topErrors.push({
      error_code: 'ACCESS_DENIED',
      outcome: 'denied',
      count: counts.denied_queries
    });
  }
  const first = {
    successful_queries: Math.floor(counts.successful_queries * 0.7),
    failed_queries: Math.floor(counts.failed_queries * 0.7),
    denied_queries: Math.floor(counts.denied_queries * 0.7)
  };
  const firstTotal = first.successful_queries + first.failed_queries + first.denied_queries;
  return {
    scope,
    period,
    from: new Date(from).toISOString(),
    to: new Date(to).toISOString(),
    bucket_seconds: bucketSeconds,
    summary: {
      ...counts,
      read_queries: counts.total_queries - writeQueries,
      write_queries: writeQueries,
      active_users: empty ? 0 : scope === 'workspace' ? 2 : 1,
      used_databases: empty ? 0 : scope === 'workspace' ? 2 : 1,
      timed_successful_queries: counts.successful_queries,
      avg_duration_ms: empty ? undefined : 128.4,
      p95_duration_ms: empty ? undefined : 342.8
    },
    series,
    top_databases: empty
      ? []
      : scope === 'personal'
        ? [{ ...counts, database_id: 'demo-analytics', name: 'Analytics · demonstração' }]
        : [
            {
              ...first,
              total_queries: firstTotal,
              database_id: 'demo-analytics',
              name: 'Analytics · demonstração'
            },
            {
              total_queries: counts.total_queries - firstTotal,
              successful_queries: counts.successful_queries - first.successful_queries,
              failed_queries: counts.failed_queries - first.failed_queries,
              denied_queries: counts.denied_queries - first.denied_queries,
              database_id: 'demo-orders',
              name: 'Pedidos · demonstração'
            }
          ],
    top_errors: topErrors,
    inventory:
      scope === 'personal'
        ? undefined
        : {
            active_databases: empty ? 0 : 2,
            paused_databases: empty ? 0 : 1,
            tls_disabled_databases: 0,
            enabled_members: 1,
            active_grants: empty ? 0 : 1,
            members_without_access: empty ? 1 : 0
          }
  };
}
