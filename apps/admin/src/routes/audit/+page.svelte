<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch, waitForSession } from '$lib/session.svelte';
  import { auditOutcomeLabel } from '$lib/presentation';
  import type { AuditEvent } from '$lib/types';

  let events = $state<AuditEvent[]>([]);
  let loading = $state(true);
  let error = $state<string | undefined>();

  onMount(() => {
    void loadEvents();
  });

  async function loadEvents(): Promise<void> {
    await waitForSession();
    try {
      const response = await apiFetch('/v1/admin/audit-events?limit=100');
      events = (await response.json()) as AuditEvent[];
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Não foi possível carregar os eventos.';
    } finally {
      loading = false;
    }
  }
</script>

<svelte:head><title>Auditoria · SQL MCP Gateway</title></svelte:head>

<div class="page-heading">
  <div>
    <p class="eyebrow">TRILHA DE SEGURANÇA</p>
    <h1>Auditoria<span class="heading-period">.</span></h1>
    <p class="page-lede">
      Ator, alvo, operação e resultado — sem gravar senhas ou valores retornados.
    </p>
  </div>
  <button class="button button-quiet" onclick={loadEvents} disabled={loading}
    >Atualizar <span aria-hidden="true">↻</span></button
  >
</div>

{#if error}<p class="notice notice-error" role="alert">{error}</p>{/if}
{#if loading}
  <div class="loading-panel">Consultando eventos recentes…</div>
{:else if events.length === 0}
  <div class="panel empty-state">
    <div class="empty-mark">≋</div>
    <strong>A trilha ainda está vazia</strong>
    <p>Eventos de concessão e operações MCP aparecerão aqui.</p>
  </div>
{:else}
  <section class="panel table-panel audit-panel">
    <div class="table-scroll">
      <table>
        <thead
          ><tr
            ><th>QUANDO</th><th>IDENTIDADE</th><th>OPERAÇÃO</th><th>ALVO</th><th>RESULTADO</th><th
              >DURAÇÃO</th
            ></tr
          ></thead
        >
        <tbody>
          {#each events as event}
            <tr>
              <td class="time-cell">{new Date(event.created_at).toLocaleString('pt-BR')}</td>
              <td
                ><strong>{event.subject}</strong><span class="cell-subtitle">{event.issuer}</span
                ></td
              >
              <td
                ><span class="mono-cell">{event.operation}</span>{#if event.query_fingerprint}<span
                    class="cell-subtitle">SHA-256 {event.query_fingerprint.slice(0, 12)}…</span
                  >{/if}</td
              >
              <td class="mono-cell">{event.database_id?.slice(0, 8) ?? '—'}</td>
              <td
                ><span
                  class="badge"
                  class:badge-live={event.outcome === 'success'}
                  class:badge-denied={event.outcome === 'denied'}
                  class:badge-muted={event.outcome === 'failure'}
                  >{auditOutcomeLabel(event.outcome)}</span
                ><span class="cell-subtitle">{event.error_code ?? ''}</span></td
              >
              <td class="mono-cell"
                >{event.duration_ms === undefined ? '—' : `${event.duration_ms} ms`}</td
              >
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <div class="audit-footnote">
      Exibindo até 100 eventos mais recentes. Fingerprints não permitem reconstruir o SQL original.
    </div>
  </section>
{/if}
