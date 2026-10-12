<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch, waitForSession } from '$lib/session.svelte';
  import { accessLevelLabel } from '$lib/presentation';
  import type { AccessLevel, DatabaseGrant, DatabaseTarget, Principal } from '$lib/types';

  let principals = $state<Principal[]>([]);
  let targets = $state<DatabaseTarget[]>([]);
  let grants = $state<DatabaseGrant[]>([]);
  let selectedPrincipalId = $state('');
  let selectedDatabaseId = $state('');
  let accessLevel = $state<AccessLevel>('read');
  let loading = $state(true);
  let saving = $state(false);
  let error = $state<string | undefined>();
  let success = $state<string | undefined>();

  onMount(() => {
    void loadPage();
  });

  async function loadPage(): Promise<void> {
    await waitForSession();
    try {
      const [principalResponse, targetResponse] = await Promise.all([
        apiFetch('/v1/admin/principals'),
        apiFetch('/v1/admin/database-targets')
      ]);
      principals = (await principalResponse.json()) as Principal[];
      targets = ((await targetResponse.json()) as DatabaseTarget[]).filter(
        (target) => target.enabled
      );
      selectedPrincipalId = principals[0]?.id ?? '';
      selectedDatabaseId = targets[0]?.id ?? '';
      await loadGrants();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Não foi possível carregar os acessos.';
    } finally {
      loading = false;
    }
  }

  async function loadGrants(): Promise<void> {
    if (!selectedPrincipalId) {
      grants = [];
      return;
    }
    const response = await apiFetch(`/v1/admin/principals/${selectedPrincipalId}/database-grants`);
    grants = (await response.json()) as DatabaseGrant[];
  }

  async function changePrincipal(event: Event): Promise<void> {
    selectedPrincipalId = (event.currentTarget as HTMLSelectElement).value;
    error = undefined;
    await loadGrants();
  }

  async function setGrant(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    error = undefined;
    success = undefined;
    saving = true;
    try {
      await apiFetch(
        `/v1/admin/principals/${selectedPrincipalId}/database-grants/${selectedDatabaseId}`,
        {
          method: 'PUT',
          body: JSON.stringify({ access_level: accessLevel })
        }
      );
      await loadGrants();
      success = 'Concessão atualizada. Ela passa a valer na próxima operação MCP.';
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Não foi possível atualizar a concessão.';
    } finally {
      saving = false;
    }
  }

  async function revoke(grant: DatabaseGrant): Promise<void> {
    error = undefined;
    success = undefined;
    try {
      await apiFetch(
        `/v1/admin/principals/${selectedPrincipalId}/database-grants/${grant.database_id}`,
        {
          method: 'DELETE'
        }
      );
      await loadGrants();
      success = `Acesso a ${grant.database_name} revogado.`;
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Não foi possível revogar o acesso.';
    }
  }
</script>

<svelte:head><title>Acessos · SQL MCP Gateway</title></svelte:head>

<div class="page-heading">
  <div>
    <p class="eyebrow">POLÍTICA POR IDENTIDADE</p>
    <h1>Acessos<span class="heading-period">.</span></h1>
    <p class="page-lede">Conceda o mínimo necessário, por pessoa e por banco.</p>
  </div>
</div>

{#if error}<p class="notice notice-error" role="alert">{error}</p>{/if}
{#if success}<p class="notice notice-success" role="status">{success}</p>{/if}

{#if loading}
  <div class="loading-panel">Carregando identidades e alvos…</div>
{:else}
  <section class="access-workspace">
    <div class="access-sidebar panel">
      <p class="eyebrow">IDENTIDADE</p>
      <label class="select-label"
        >Pessoa
        <select value={selectedPrincipalId} onchange={changePrincipal}>
          {#each principals as principal}
            <option value={principal.id}
              >{principal.display_name ??
                principal.email ??
                principal.subject}{principal.platform_role === 'admin' ? ' · admin' : ''}</option
            >
          {/each}
        </select>
      </label>
      {#if principals.length === 0}<div class="empty-inline">
          Ninguém entrou pelo SSO ainda.
        </div>{/if}
      <p class="microcopy">
        A identidade é a combinação exata do emissor OIDC e do subject — não do endereço de e-mail.
      </p>
    </div>

    <div class="access-main panel">
      <div class="panel-heading">
        <div>
          <p class="eyebrow">CONCESSÕES EXPLÍCITAS</p>
          <h2>Permissões de banco</h2>
        </div>
        <span class="count-pill">{grants.length.toString().padStart(2, '0')}</span>
      </div>

      {#if grants.length === 0}
        <div class="empty-state compact-empty">
          <div class="empty-mark">⊞</div>
          <strong>Nenhum acesso concedido</strong>
          <p>Essa pessoa não consegue enumerar nem consultar bancos.</p>
        </div>
      {:else}
        <div class="grant-list">
          {#each grants as grant}
            <div class="grant-row">
              <div class="grant-database">
                <span class="engine-mark">DB</span>
                <div>
                  <strong>{grant.database_name}</strong><span
                    >Concedido em {new Date(grant.granted_at ?? '').toLocaleDateString(
                      'pt-BR'
                    )}</span
                  >
                </div>
              </div>
              <span class="badge" class:badge-write={grant.access_level === 'write'}
                >{accessLevelLabel(grant.access_level)}</span
              >
              <button class="text-button text-danger" onclick={() => revoke(grant)}>Revogar</button>
            </div>
          {/each}
        </div>
      {/if}

      <form class="grant-form" onsubmit={setGrant}>
        <div>
          <p class="eyebrow">NOVA CONCESSÃO</p>
          <h3>Adicionar ou alterar acesso</h3>
        </div>
        <div class="form-row">
          <label
            >Banco
            <select bind:value={selectedDatabaseId} required>
              {#each targets as target}<option value={target.id}
                  >{target.name} · {target.engine}</option
                >{/each}
            </select>
          </label>
          <label
            >Nível
            <select bind:value={accessLevel}
              ><option value="read">Leitura</option><option value="write"
                >Leitura e escrita (DML)</option
              ></select
            >
          </label>
        </div>
        <button
          class="button button-primary"
          type="submit"
          disabled={saving || !selectedPrincipalId || !selectedDatabaseId}
        >
          {saving ? 'Salvando…' : 'Aplicar concessão'}<span aria-hidden="true">→</span>
        </button>
        <p class="microcopy">
          Escrita permite INSERT, UPDATE e DELETE. DDL e administração continuam bloqueados.
        </p>
      </form>
    </div>
  </section>
{/if}
