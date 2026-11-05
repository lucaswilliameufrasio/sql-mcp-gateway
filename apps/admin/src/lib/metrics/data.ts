import { apiFetch } from '$lib/session.svelte';
import type {
  MetricsInventory,
  MetricsPeriod,
  MetricsScope,
  MetricsSnapshot,
  OutcomeCounts
} from './model';

function invalid(): never {
  throw new Error(
    'O gateway retornou métricas inválidas. Atualize a página ou contate o administrador.'
  );
}
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
function record(value: unknown): Record<string, unknown> {
  if (!isRecord(value)) {
    return invalid();
  }
  return value;
}
function list(value: unknown, max: number): unknown[] {
  if (!Array.isArray(value) || value.length > max) {
    return invalid();
  }
  return value;
}
function count(value: unknown): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0) {
    return invalid();
  }
  return value;
}
function text(value: unknown): string {
  if (typeof value !== 'string' || !value || value.length > 1024) {
    return invalid();
  }
  return value;
}
function timestamp(value: unknown): string {
  const result = text(value);
  if (!result.endsWith('Z') || !Number.isFinite(Date.parse(result))) {
    return invalid();
  }
  return result;
}
function duration(value: unknown): number | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (typeof value !== 'number' || !Number.isFinite(value) || value < 0) {
    return invalid();
  }
  return value;
}
function outcomes(value: Record<string, unknown>): OutcomeCounts {
  const result = {
    total_queries: count(value.total_queries),
    successful_queries: count(value.successful_queries),
    failed_queries: count(value.failed_queries),
    denied_queries: count(value.denied_queries)
  };
  if (
    result.successful_queries + result.failed_queries + result.denied_queries !==
    result.total_queries
  ) {
    return invalid();
  }
  return result;
}
function inventory(value: unknown): MetricsInventory {
  const row = record(value);
  return {
    active_databases: count(row.active_databases),
    paused_databases: count(row.paused_databases),
    tls_disabled_databases: count(row.tls_disabled_databases),
    enabled_members: count(row.enabled_members),
    active_grants: count(row.active_grants),
    members_without_access: count(row.members_without_access)
  };
}

export function parseMetrics(
  value: unknown,
  period: MetricsPeriod,
  scope: MetricsScope
): MetricsSnapshot {
  const row = record(value);
  if (row.period !== period || row.scope !== scope) {
    return invalid();
  }
  const from = timestamp(row.from);
  const to = timestamp(row.to);
  const bucketSeconds = period === '24h' ? 3600 : 86400;
  const bucketCount = period === '24h' ? 24 : period === '7d' ? 7 : 30;
  if (
    row.bucket_seconds !== bucketSeconds ||
    Date.parse(to) - Date.parse(from) !== bucketSeconds * bucketCount * 1000
  ) {
    return invalid();
  }
  const summaryRow = record(row.summary);
  const summary = {
    ...outcomes(summaryRow),
    read_queries: count(summaryRow.read_queries),
    write_queries: count(summaryRow.write_queries),
    active_users: count(summaryRow.active_users),
    used_databases: count(summaryRow.used_databases),
    timed_successful_queries: count(summaryRow.timed_successful_queries),
    avg_duration_ms: duration(summaryRow.avg_duration_ms),
    p95_duration_ms: duration(summaryRow.p95_duration_ms)
  };
  if (
    summary.read_queries + summary.write_queries !== summary.total_queries ||
    summary.timed_successful_queries > summary.successful_queries ||
    summary.active_users > summary.total_queries ||
    (scope === 'personal' && summary.active_users > 1) ||
    summary.used_databases > summary.total_queries ||
    summary.timed_successful_queries > 0 !==
      (summary.avg_duration_ms !== undefined && summary.p95_duration_ms !== undefined) ||
    (summary.timed_successful_queries === 0 &&
      (summary.avg_duration_ms !== undefined || summary.p95_duration_ms !== undefined))
  ) {
    return invalid();
  }
  const series = list(row.series, bucketCount).map((value, index) => {
    const bucket = record(value);
    const start = timestamp(bucket.start);
    if (Date.parse(start) !== Date.parse(from) + index * bucketSeconds * 1000) {
      return invalid();
    }
    return { ...outcomes(bucket), start };
  });
  if (series.length !== bucketCount) {
    return invalid();
  }
  const countKeys: (keyof OutcomeCounts)[] = [
    'total_queries',
    'successful_queries',
    'failed_queries',
    'denied_queries'
  ];
  for (const key of countKeys) {
    const total = series.reduce((sum, bucket) => sum + bucket[key], 0);
    if (total !== summary[key]) {
      return invalid();
    }
  }
  if (scope === 'personal' && row.inventory !== undefined) {
    return invalid();
  }
  return {
    scope,
    period,
    from,
    to,
    bucket_seconds: bucketSeconds,
    summary,
    series,
    top_databases: list(row.top_databases, 5).map((value) => {
      const database = record(value);
      const result = {
        ...outcomes(database),
        database_id: text(database.database_id),
        name: text(database.name)
      };
      if (result.total_queries === 0 || result.total_queries > summary.total_queries) {
        return invalid();
      }
      return result;
    }),
    top_errors: list(row.top_errors, 5).map((value) => {
      const error = record(value);
      if (error.outcome !== 'failure' && error.outcome !== 'denied') {
        return invalid();
      }
      const occurrences = count(error.count);
      const maximum = error.outcome === 'failure' ? summary.failed_queries : summary.denied_queries;
      if (occurrences === 0 || occurrences > maximum) {
        return invalid();
      }
      return {
        error_code: text(error.error_code),
        outcome: error.outcome,
        count: occurrences
      };
    }),
    inventory: scope === 'workspace' ? inventory(row.inventory) : undefined
  };
}

export async function loadUsageMetrics(
  period: MetricsPeriod,
  scope: MetricsScope,
  signal: AbortSignal
): Promise<MetricsSnapshot> {
  const response = await apiFetch(`/v1/metrics?period=${period}`, { signal });
  const value: unknown = await response.json();
  return parseMetrics(value, period, scope);
}
