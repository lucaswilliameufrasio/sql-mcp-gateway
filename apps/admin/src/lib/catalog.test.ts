import { describe, expect, it } from 'vitest';
import { filterAuditEvents, filterTargets } from './catalog';
import type { AuditEvent, DatabaseTarget } from './types';

const targets: DatabaseTarget[] = [
  {
    id: '1',
    name: 'Análise',
    engine: 'postgresql',
    host: 'warehouse.internal',
    port: 5432,
    database_name: 'analytics',
    schema_name: 'public',
    tls_mode: 'verify_full',
    enabled: true,
    created_at: '2026-11-05T00:00:00Z'
  },
  {
    id: '2',
    name: 'Pedidos',
    engine: 'mysql',
    host: 'orders.internal',
    port: 3306,
    database_name: 'commerce',
    schema_name: 'commerce',
    tls_mode: 'disable',
    enabled: false,
    created_at: '2026-11-05T00:00:00Z'
  }
];

describe('database catalog filters', () => {
  it('searches case-insensitively by display name, database, host, and engine', () => {
    for (const query of [' ANÁLISE ', 'ANALYTICS', 'WAREHOUSE', 'POSTGRESQL']) {
      expect(filterTargets(targets, query, 'all').map((target) => target.id)).toEqual(['1']);
    }
  });
  it('combines the search with enabled state without mutating the source', () => {
    expect(filterTargets(targets, '', 'active').map((target) => target.id)).toEqual(['1']);
    expect(filterTargets(targets, 'mysql', 'paused').map((target) => target.id)).toEqual(['2']);
    expect(filterTargets(targets, 'mysql', 'active')).toEqual([]);
    expect(targets).toHaveLength(2);
  });
  it('distinguishes empty collections and no matches', () => {
    expect(filterTargets([], '', 'all')).toEqual([]);
    expect(filterTargets(targets, 'missing', 'all')).toEqual([]);
    expect(filterTargets(targets, '', 'all')).toHaveLength(2);
  });
});

const events: AuditEvent[] = [
  {
    id: 'a',
    issuer: 'https://id.example.test',
    subject: 'maria',
    operation: 'query',
    database_id: 'bank-1',
    outcome: 'success',
    created_at: '2026-11-05T00:00:00Z'
  },
  {
    id: 'b',
    issuer: 'https://id.example.test',
    subject: 'joao',
    operation: 'execute',
    outcome: 'denied',
    error_code: 'access_denied',
    created_at: '2026-11-05T00:00:00Z'
  },
  {
    id: 'c',
    issuer: 'https://id.example.test',
    subject: 'maria',
    operation: 'query',
    outcome: 'failure',
    created_at: '2026-11-05T00:00:00Z'
  }
];

describe('audit filters', () => {
  it('combines outcome and case-insensitive identity search', () => {
    expect(filterAuditEvents(events, ' MARIA ', 'failure').map((event) => event.id)).toEqual(['c']);
    expect(filterAuditEvents(events, 'maria', 'denied')).toEqual([]);
  });
  it('searches operations, database identifiers, and safe error codes', () => {
    expect(filterAuditEvents(events, 'BANK-1', 'all').map((event) => event.id)).toEqual(['a']);
    expect(filterAuditEvents(events, 'EXECUTE', 'all').map((event) => event.id)).toEqual(['b']);
    expect(filterAuditEvents(events, 'ACCESS_DENIED', 'all').map((event) => event.id)).toEqual([
      'b'
    ]);
  });
  it('handles optional metadata and returns the loaded collection when filters are cleared', () => {
    expect(filterAuditEvents(events, '', 'all')).toHaveLength(3);
    expect(filterAuditEvents([], '', 'all')).toEqual([]);
    expect(filterAuditEvents(events, 'undefined', 'all')).toEqual([]);
  });
});
