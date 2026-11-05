<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch, session, waitForSession } from '$lib/session.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import { filterAuditEvents } from '$lib/catalog';
  import { auditOutcomeLabel } from '$lib/presentation';
  import type { AuditEvent } from '$lib/types';

  let events = $state<AuditEvent[]>([]);
  let loading = $state(true);
  let error = $state<string | undefined>();
  let search = $state('');
  let outcome = $state('all');
  let filtered = $derived(filterAuditEvents(events, search, outcome));

  onMount(() => {
    void loadEvents();
  });

  async function loadEvents(): Promise<void> {
    loading = true;
    error = undefined;
    await waitForSession();
    if (session.currentPrincipal?.platform_role !== 'admin') {
      loading = false;
      return;
    }
    try {
      const response = await apiFetch('/v1/admin/audit-events?limit=100');
      events = await response.json();
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
    <h1>Auditoria</h1>
    <p class="page-lede">
      Ator, alvo, operação e resultado — sem gravar senhas ou valores retornados.
    </p>
  </div>
  <button class="button button-quiet" onclick={loadEvents} disabled={loading}
    ><Icon name="refresh" size={17} />{loading ? 'Atualizando…' : 'Atualizar'}</button
  >
</div>

{#if loading}
  <div class="loading-panel" role="status">Consultando eventos recentes…</div>
{:else if session.currentPrincipal?.platform_role !== 'admin'}<div class="notice">
    A auditoria é restrita a administradores. <a class="text-link" href="/">Voltar ao início</a>
  </div>
{:else if error}<div class="notice notice-error" role="alert">
    <p>{error}</p>
    <button class="button button-secondary" onclick={loadEvents}>Tentar novamente</button>
  </div>
{:else if events.length === 0}
  <div class="panel empty-state">
    <Icon name="audit" size={34} />
    <h2>As operações aparecerão aqui</h2>
    <p>Concessões de acesso e operações MCP geram eventos. Volte após a primeira atividade.</p>
    <a class="button button-secondary" href="/access">Gerenciar acessos</a>
  </div>
{:else}
  <section class="panel table-panel audit-panel">
    <div class="filter-toolbar">
      <label class="search-field"
        ><Icon name="search" size={18} /><input
          type="search"
          aria-label="Buscar eventos"
          bind:value={search}
          placeholder="Buscar pessoa, operação ou banco"
        /></label
      ><label class="filter-select"
        >Resultado<select bind:value={outcome}
          ><option value="all">Todos</option><option value="success">Concluído</option><option
            value="denied">Negado</option
          ><option value="failure">Falhou</option></select
        ></label
      ><span class="result-count" role="status">{filtered.length} de {events.length}</span>
    </div>
    {#if filtered.length === 0}<div class="empty-state">
        <Icon name="search" size={30} />
        <h3>Nenhum evento encontrado</h3>
        <p>Os filtros se aplicam apenas aos 100 eventos mais recentes.</p>
        <button
          class="button button-secondary"
          onclick={() => {
            search = '';
            outcome = 'all';
          }}>Limpar filtros</button
        >
      </div>{:else}
      <div class="table-scroll">
        <table>
          <caption class="sr-only">Últimos eventos de acesso e operações MCP</caption>
          <thead
            ><tr
              ><th scope="col">Quando</th><th scope="col">Pessoa</th><th scope="col">Operação</th
              ><th scope="col">Banco</th><th scope="col">Resultado</th><th scope="col">Duração</th
              ></tr
            ></thead
          >
          <tbody>
            {#each filtered as event}
              <tr>
                <td class="time-cell">{new Date(event.created_at).toLocaleString('pt-BR')}</td>
                <td
                  ><strong>{event.subject}</strong><span class="cell-subtitle">{event.issuer}</span
                  ></td
                >
                <td
                  ><span class="mono-cell">{event.operation}</span
                  >{#if event.query_fingerprint}<span class="cell-subtitle"
                      >SHA-256 {event.query_fingerprint.slice(0, 12)}…</span
                    >{/if}</td
                >
                <td class="mono-cell">{event.database_id?.slice(0, 8) ?? '—'}</td>
                <td
                  ><span
                    class="badge"
                    class:badge-live={event.outcome === 'success'}
                    class:badge-denied={event.outcome === 'denied'}
                    class:badge-error={event.outcome === 'failure'}
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
    {/if}
    <div class="audit-footnote">
      Exibindo até 100 eventos mais recentes. Fingerprints não permitem reconstruir o SQL original.
    </div>
  </section>
{/if}
