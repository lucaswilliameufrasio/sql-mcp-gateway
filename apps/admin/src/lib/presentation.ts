import type { AccessLevel, AuditEvent } from './types';

export function accessLevelLabel(level: AccessLevel): string {
  return level === 'write' ? 'Leitura + escrita' : 'Somente leitura';
}

export function auditOutcomeLabel(outcome: AuditEvent['outcome']): string {
  switch (outcome) {
    case 'success':
      return 'Concluído';
    case 'denied':
      return 'Negado';
    case 'failure':
      return 'Falhou';
  }
}
