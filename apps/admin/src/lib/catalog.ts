import type { AuditEvent, DatabaseTarget } from './types';

export function filterTargets(
  targets: DatabaseTarget[],
  search: string,
  status: string
): DatabaseTarget[] {
  const query = search.trim().toLocaleLowerCase('pt-BR');
  return targets.filter((target) => {
    const matchesStatus =
      status === 'all' || (status === 'active' ? target.enabled : !target.enabled);
    const text =
      `${target.name} ${target.database_name} ${target.host} ${target.engine}`.toLocaleLowerCase(
        'pt-BR'
      );
    return matchesStatus && text.includes(query);
  });
}

export function filterAuditEvents(
  events: AuditEvent[],
  search: string,
  outcome: string
): AuditEvent[] {
  const query = search.trim().toLocaleLowerCase('pt-BR');
  return events.filter((event) => {
    const text =
      `${event.subject} ${event.operation} ${event.database_id ?? ''} ${event.error_code ?? ''}`.toLocaleLowerCase(
        'pt-BR'
      );
    return (outcome === 'all' || event.outcome === outcome) && text.includes(query);
  });
}
