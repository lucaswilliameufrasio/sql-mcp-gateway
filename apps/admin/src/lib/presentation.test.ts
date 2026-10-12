import { describe, expect, it } from 'vitest';
import { accessLevelLabel, auditOutcomeLabel } from './presentation';

describe('accessLevelLabel', () => {
  it('uses plain language for read and write grants', () => {
    expect(accessLevelLabel('read')).toBe('Somente leitura');
    expect(accessLevelLabel('write')).toBe('Leitura + escrita');
  });
});

describe('auditOutcomeLabel', () => {
  it('distinguishes successful, denied, and failed events', () => {
    expect(auditOutcomeLabel('success')).toBe('Concluído');
    expect(auditOutcomeLabel('denied')).toBe('Negado');
    expect(auditOutcomeLabel('failure')).toBe('Falhou');
  });
});
