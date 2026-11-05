export type MetricsPeriod = '24h' | '7d' | '30d';
export type MetricsScope = 'workspace' | 'personal';
export interface OutcomeCounts {
  total_queries: number;
  successful_queries: number;
  failed_queries: number;
  denied_queries: number;
}
export interface MetricsSummary extends OutcomeCounts {
  read_queries: number;
  write_queries: number;
  active_users: number;
  used_databases: number;
  timed_successful_queries: number;
  avg_duration_ms?: number;
  p95_duration_ms?: number;
}
export interface MetricsBucket extends OutcomeCounts {
  start: string;
}
export interface DatabaseUsage extends OutcomeCounts {
  database_id: string;
  name: string;
}
export interface MetricsInventory {
  active_databases: number;
  paused_databases: number;
  tls_disabled_databases: number;
  enabled_members: number;
  active_grants: number;
  members_without_access: number;
}
export interface MetricsSnapshot {
  scope: MetricsScope;
  period: MetricsPeriod;
  from: string;
  to: string;
  bucket_seconds: number;
  summary: MetricsSummary;
  series: MetricsBucket[];
  top_databases: DatabaseUsage[];
  top_errors: { error_code: string; outcome: 'failure' | 'denied'; count: number }[];
  inventory?: MetricsInventory;
}

export function successRate(summary: OutcomeCounts): number | undefined {
  if (summary.total_queries === 0) {
    return undefined;
  }
  return (summary.successful_queries / summary.total_queries) * 100;
}

export function trendPath(buckets: MetricsBucket[]): string {
  if (buckets.length < 2) {
    return '';
  }
  const max = Math.max(1, ...buckets.map((bucket) => bucket.total_queries));
  return buckets
    .map((bucket, index) => {
      const x = (index / (buckets.length - 1)) * 720;
      const y = 170 - (bucket.total_queries / max) * 150;
      return `${index === 0 ? 'M' : 'L'}${x.toFixed(2)},${y.toFixed(2)}`;
    })
    .join(' ');
}

export function metricNumber(value: number): string {
  return value.toLocaleString('pt-BR', { maximumFractionDigits: 1 });
}

export function metricTime(value: string, hourly = false): string {
  return new Date(value).toLocaleString(
    'pt-BR',
    hourly
      ? { timeZone: 'UTC', day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit' }
      : { timeZone: 'UTC', day: '2-digit', month: '2-digit' }
  );
}

export function metricErrorLabel(code: string): string {
  switch (code) {
    case 'ACCESS_DENIED':
      return 'Permissão insuficiente';
    case 'QUERY_TIMEOUT':
      return 'Tempo limite da operação';
    case 'INVALID_QUERY':
      return 'Consulta não permitida';
    case 'DATABASE_OPERATION_FAILED':
      return 'Falha na operação do banco';
    case 'CREDENTIAL_BROKER_UNAVAILABLE':
      return 'Credencial temporária indisponível';
    case 'UNCLASSIFIED':
      return 'Sem classificação';
    default:
      return code;
  }
}
