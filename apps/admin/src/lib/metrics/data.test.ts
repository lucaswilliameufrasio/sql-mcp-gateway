import { describe, expect, it, vi } from 'vitest';
import { metricsFixture } from '../../../tests/fixtures/metrics';
import { parseMetrics, loadUsageMetrics } from './data';
import { apiFetch } from '$lib/session.svelte';
import { metricErrorLabel, successRate, trendPath } from './model';

vi.mock('$lib/session.svelte', () => ({ apiFetch: vi.fn() }));

describe('usage metrics contract', () => {
  it('validates all periods and identity scopes against the requested contract', () => {
    for (const period of ['24h', '7d', '30d'] as const) {
      for (const scope of ['workspace', 'personal'] as const) {
        const fixture = metricsFixture(period, scope);
        const parsed = parseMetrics(JSON.parse(JSON.stringify(fixture)), period, scope);
        expect(parsed.summary.total_queries).toBeGreaterThan(0);
        expect(parsed.scope).toBe(scope);
        expect(parsed.period).toBe(period);
      }
    }
  });
  it('preserves zero activity without claiming 100% success or zero latency', () => {
    const empty = parseMetrics(
      JSON.parse(JSON.stringify(metricsFixture('24h', 'personal', true))),
      '24h',
      'personal'
    );
    expect(empty.summary.total_queries).toBe(0);
    expect(empty.summary.avg_duration_ms).toBeUndefined();
    expect(empty.summary.p95_duration_ms).toBeUndefined();
    expect(successRate(empty.summary)).toBeUndefined();
    expect(trendPath(empty.series)).not.toMatch(/NaN|Infinity/);
  });
  it('rejects workspace data for members instead of exposing it as personal metrics', () => {
    expect(() => parseMetrics(metricsFixture(), '24h', 'personal')).toThrow('métricas inválidas');
    const leaked = { ...metricsFixture('24h', 'personal'), inventory: metricsFixture().inventory };
    expect(() => parseMetrics(leaked, '24h', 'personal')).toThrow('métricas inválidas');
  });
  it('rejects incorrect counts, missing data, nulls, and nonfinite timing', () => {
    const fixture = metricsFixture();
    const malformed: unknown[] = [
      undefined,
      null,
      {},
      [],
      { ...fixture, summary: { ...fixture.summary, total_queries: -1 } },
      { ...fixture, summary: { ...fixture.summary, total_queries: '123' } },
      { ...fixture, summary: { ...fixture.summary, p95_duration_ms: null } },
      { ...fixture, summary: { ...fixture.summary, avg_duration_ms: Infinity } },
      {
        ...fixture,
        summary: { ...fixture.summary, timed_successful_queries: fixture.summary.total_queries + 1 }
      },
      { ...fixture, series: [] },
      { ...fixture, series: fixture.series.slice(1) },
      { ...fixture, bucket_seconds: 86400 },
      { ...fixture, to: fixture.from },
      { ...fixture, top_errors: [{ error_code: 'x', outcome: 'other', count: 1 }] },
      {
        ...fixture,
        top_errors: [
          { error_code: 'x', outcome: 'denied', count: fixture.summary.total_queries + 1 }
        ]
      },
      {
        ...fixture,
        top_databases: [
          {
            database_id: 'zero',
            name: 'Zero',
            total_queries: 0,
            successful_queries: 0,
            failed_queries: 0,
            denied_queries: 0
          }
        ]
      }
    ];
    for (const input of malformed) {
      expect(() => parseMetrics(input, '24h', 'workspace')).toThrow('métricas inválidas');
    }
  });
  it('rejects summary/series disagreement and shifted time buckets', () => {
    const fixture = metricsFixture();
    expect(() =>
      parseMetrics(
        {
          ...fixture,
          series: fixture.series.map((bucket, index) =>
            index === 0 ? { ...bucket, start: fixture.to } : bucket
          )
        },
        '24h',
        'workspace'
      )
    ).toThrow();
    expect(() =>
      parseMetrics(
        {
          ...fixture,
          series: fixture.series.map((bucket, index) =>
            index === 0
              ? {
                  ...bucket,
                  total_queries: bucket.total_queries + 1,
                  successful_queries: bucket.successful_queries + 1
                }
              : bucket
          )
        },
        '24h',
        'workspace'
      )
    ).toThrow();
  });
  it('loads a named bounded operation and validates its response', async () => {
    vi.mocked(apiFetch).mockResolvedValueOnce(Response.json(metricsFixture('7d', 'personal')));
    const signal = new AbortController().signal;
    const data = await loadUsageMetrics('7d', 'personal', signal);
    expect(apiFetch).toHaveBeenLastCalledWith('/v1/metrics?period=7d', { signal });
    expect(data.series).toHaveLength(7);
    vi.mocked(apiFetch).mockRejectedValueOnce(new Error('Serviço indisponível'));
    await expect(loadUsageMetrics('24h', 'personal', signal)).rejects.toThrow(
      'Serviço indisponível'
    );
  });
});

describe('metrics presentation', () => {
  it('uses every audited attempt in the success denominator', () => {
    expect(
      successRate({
        total_queries: 10,
        successful_queries: 8,
        failed_queries: 1,
        denied_queries: 1
      })
    ).toBe(80);
  });
  it('produces finite trend geometry with UTC bucket order', () => {
    const data = metricsFixture();
    const path = trendPath(data.series);
    expect(path.startsWith('M0.00,')).toBe(true);
    expect(path).toContain('L720.00,');
    expect(path).not.toMatch(/NaN|Infinity/);
    expect(trendPath([])).toBe('');
  });
  it('uses safe plain-language explanations without hiding unknown codes', () => {
    expect(metricErrorLabel('QUERY_TIMEOUT')).toBe('Tempo limite da operação');
    expect(metricErrorLabel('CUSTOM_ERROR')).toBe('CUSTOM_ERROR');
  });
});
