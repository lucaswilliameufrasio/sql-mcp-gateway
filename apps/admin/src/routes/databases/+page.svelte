<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch, session, waitForSession } from '$lib/session.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
  import { filterTargets } from '$lib/catalog';
  import type { DatabaseTarget } from '$lib/types';

  let targets = $state<DatabaseTarget[]>([]);
  let loading = $state(true);
  let busy = $state(false);
  let error = $state<string | undefined>();
  let actionError = $state<string | undefined>();
  let success = $state<string | undefined>();
  let search = $state('');
  let status = $state('all');
  let dialog = $state<{ type: 'none' } | { type: 'pause'; target: DatabaseTarget }>({
    type: 'none'
  });
  let filtered = $derived(filterTargets(targets, search, status));

  onMount(() => {
    void loadTargets();
  });
  async function loadTargets(): Promise<void> {
    loading = true;
    error = undefined;
    await waitForSession();
    if (session.currentPrincipal?.platform_role !== 'admin') {
      loading = false;
      return;
    }
    try {
      const response = await apiFetch('/v1/admin/database-targets');
      targets = await response.json();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Não foi possível carregar os bancos.';
    } finally {
      loading = false;
    }
  }
  async function setEnabled(target: DatabaseTarget): Promise<void> {
    busy = true;
    actionError = undefined;
    success = undefined;
    try {
      await apiFetch(`/v1/admin/database-targets/${target.id}/enabled`, {
        method: 'PUT',
        body: JSON.stringify({ enabled: !target.enabled })
      });
      targets = targets.map((item) =>
        item.id === target.id ? { ...item, enabled: !target.enabled } : item
      );
      success = `${target.name}: banco ${target.enabled ? 'pausado' : 'ativado'}.`;
      dialog = { type: 'none' };
    } catch (cause) {
      actionError = cause instanceof Error ? cause.message : 'Não foi possível alterar o banco.';
    } finally {
      busy = false;
    }
  }
</script>

<svelte:head><title>Bancos de dados · SQL MCP Gateway</title></svelte:head>
<div class="page-heading">
  <div>
    <h1>Bancos de dados</h1>
    <p class="page-lede">Gerencie as conexões disponíveis para acesso via MCP.</p>
  </div>
  {#if session.currentPrincipal?.platform_role === 'admin'}<a
      class="button button-primary"
      href="/databases/new"><Icon name="plus" size={18} />Cadastrar banco</a
    >{/if}
</div>
{#if loading}<div class="loading-panel" role="status">Carregando bancos…</div>
{:else if session.currentPrincipal?.platform_role !== 'admin'}<div class="notice">
    O catálogo administrativo é restrito a administradores. <a class="text-link" href="/"
      >Ver meus acessos</a
    >
  </div>
{:else if error}<div class="notice notice-error" role="alert">
    <p>{error}</p>
    <button class="button button-secondary" onclick={loadTargets}>Tentar novamente</button>
  </div>
{:else}
  {#if success}<p class="notice notice-success" role="status">{success}</p>{/if}
  {#if actionError && dialog.type === 'none'}<p class="notice notice-error" role="alert">
      {actionError}
    </p>{/if}
  <section class="panel table-panel">
    <div class="panel-heading">
      <div>
        <h2>Catálogo de conexões <span class="count-pill">{targets.length}</span></h2>
        <p class="section-description">
          Pausar um banco bloqueia novas operações. As concessões são mantidas.
        </p>
      </div>
    </div>
    {#if targets.length === 0}<div class="empty-state">
        <Icon name="database" size={34} />
        <h3>Adicione seu primeiro banco</h3>
        <p>Conecte um PostgreSQL ou MySQL e depois conceda acesso à equipe.</p>
        <a class="button button-primary" href="/databases/new">Cadastrar banco</a>
      </div>
    {:else}
      <div class="filter-toolbar">
        <label class="search-field"
          ><Icon name="search" size={18} /><input
            type="search"
            aria-label="Buscar bancos"
            bind:value={search}
            placeholder="Buscar por nome, banco ou host"
          /></label
        ><label class="filter-select"
          >Estado<select bind:value={status}
            ><option value="all">Todos</option><option value="active">Ativos</option><option
              value="paused">Pausados</option
            ></select
          ></label
        ><span class="result-count" role="status">{filtered.length} de {targets.length}</span>
      </div>
      {#if filtered.length === 0}<div class="empty-state">
          <Icon name="search" size={30} />
          <h3>Nenhum banco encontrado</h3>
          <p>Tente outro nome ou remova o filtro de estado.</p>
          <button
            class="button button-secondary"
            onclick={() => {
              search = '';
              status = 'all';
            }}>Limpar filtros</button
          >
        </div>
      {:else}<div class="table-scroll">
          <table>
            <caption class="sr-only">Bancos cadastrados e estado de cada conexão</caption><thead
              ><tr
                ><th scope="col">Banco</th><th scope="col">Conexão</th><th scope="col">Segurança</th
                ><th scope="col">Estado</th><th scope="col"><span class="sr-only">Ações</span></th
                ></tr
              ></thead
            ><tbody
              >{#each filtered as target}<tr
                  ><td
                    ><div class="database-cell">
                      <span class="engine-mark" class:engine-mysql={target.engine === 'mysql'}
                        >{target.engine === 'mysql' ? 'My' : 'Pg'}</span
                      >
                      <div>
                        <strong>{target.name}</strong><span class="cell-subtitle"
                          >{target.engine === 'mysql' ? 'MySQL' : 'PostgreSQL'} · {target.database_name}</span
                        >
                      </div>
                    </div></td
                  ><td
                    ><code>{target.host}:{target.port}</code><span class="cell-subtitle"
                      >Schema: {target.schema_name}</span
                    ></td
                  ><td
                    ><span class="badge" class:badge-warning={target.tls_mode === 'disable'}
                      >{target.tls_mode === 'disable' ? 'Sem TLS' : 'TLS verificado'}</span
                    ></td
                  ><td
                    ><span
                      class="badge"
                      class:badge-live={target.enabled}
                      class:badge-muted={!target.enabled}
                      >{target.enabled ? 'Ativo' : 'Pausado'}</span
                    ></td
                  ><td
                    ><button
                      class="button button-quiet button-small"
                      disabled={busy}
                      aria-label={`${target.enabled ? 'Pausar' : 'Ativar'} ${target.name}`}
                      onclick={() => {
                        if (target.enabled) {
                          actionError = undefined;
                          dialog = { type: 'pause', target };
                        } else {
                          void setEnabled(target);
                        }
                      }}>{target.enabled ? 'Pausar' : 'Ativar'}</button
                    ></td
                  ></tr
                >{/each}</tbody
            >
          </table>
        </div>{/if}
    {/if}
  </section>
  <p class="page-footnote">
    <Icon name="users" size={16} /> Conexão cadastrada?
    <a href="/access">Conceda acesso à equipe</a> para liberar o uso pelo MCP.
  </p>
{/if}
{#if dialog.type === 'pause'}
  {@const target = dialog.target}
  <ConfirmDialog
    title={`Pausar ${target.name}?`}
    description="Novas operações MCP neste banco serão bloqueadas. As permissões existentes serão mantidas e você poderá reativar a conexão depois."
    action="Pausar banco"
    {busy}
    error={actionError}
    oncancel={() => {
      dialog = { type: 'none' };
    }}
    onconfirm={() => {
      void setEnabled(target);
    }}
  />
{/if}
