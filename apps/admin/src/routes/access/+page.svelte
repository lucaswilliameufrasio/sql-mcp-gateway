<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch, session, waitForSession } from '$lib/session.svelte';
  import { accessLevelLabel } from '$lib/presentation';
  import Icon from '$lib/components/Icon.svelte';
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
  import type { AccessLevel, DatabaseGrant, DatabaseTarget, Principal } from '$lib/types';

  let principals = $state<Principal[]>([]);
  let targets = $state<DatabaseTarget[]>([]);
  let grants = $state<DatabaseGrant[]>([]);
  let selectedPrincipalId = $state('');
  let selectedDatabaseId = $state('');
  let accessLevel = $state<AccessLevel>('read');
  let loading = $state(true);
  let grantsLoading = $state(false);
  let saving = $state(false);
  let error = $state<string | undefined>();
  let grantError = $state<string | undefined>();
  let actionError = $state<string | undefined>();
  let success = $state<string | undefined>();
  let dialog = $state<
    { type: 'none' } | { type: 'revoke'; grant: DatabaseGrant; principalId: string }
  >({ type: 'none' });
  let selectedPerson = $derived(
    principals.find((principal) => principal.id === selectedPrincipalId)
  );
  let selectedTarget = $derived(targets.find((target) => target.id === selectedDatabaseId));
  let existingGrant = $derived(grants.find((grant) => grant.database_id === selectedDatabaseId));
  let grantRequest = 0;

  onMount(() => {
    void loadPage();
  });
  async function loadPage(): Promise<void> {
    loading = true;
    error = undefined;
    await waitForSession();
    if (session.currentPrincipal?.platform_role !== 'admin') {
      loading = false;
      return;
    }
    try {
      const [principalResponse, targetResponse] = await Promise.all([
        apiFetch('/v1/admin/principals'),
        apiFetch('/v1/admin/database-targets')
      ]);
      principals = await principalResponse.json();
      const allTargets: DatabaseTarget[] = await targetResponse.json();
      targets = allTargets.filter((target) => target.enabled);
      selectedPrincipalId = principals[0]?.id ?? '';
      selectedDatabaseId = targets[0]?.id ?? '';
      await loadGrants();
    } catch (cause) {
      error =
        cause instanceof Error ? cause.message : 'Não foi possível carregar pessoas e bancos.';
    } finally {
      loading = false;
    }
  }
  async function loadGrants(): Promise<void> {
    const request = ++grantRequest;
    const principalId = selectedPrincipalId;
    grants = [];
    grantError = undefined;
    if (!principalId) {
      return;
    }
    grantsLoading = true;
    try {
      const response = await apiFetch(`/v1/admin/principals/${principalId}/database-grants`);
      const result: DatabaseGrant[] = await response.json();
      if (request === grantRequest) {
        grants = result;
      }
    } catch (cause) {
      if (request === grantRequest) {
        grantError =
          cause instanceof Error ? cause.message : 'Não foi possível carregar as permissões.';
      }
    } finally {
      if (request === grantRequest) {
        grantsLoading = false;
      }
    }
  }
  async function changePrincipal(): Promise<void> {
    success = undefined;
    actionError = undefined;
    accessLevel = 'read';
    await loadGrants();
  }
  async function setGrant(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!selectedPrincipalId || !selectedDatabaseId || saving || grantsLoading || grantError) {
      return;
    }
    actionError = undefined;
    success = undefined;
    saving = true;
    try {
      await apiFetch(
        `/v1/admin/principals/${selectedPrincipalId}/database-grants/${selectedDatabaseId}`,
        { method: 'PUT', body: JSON.stringify({ access_level: accessLevel }) }
      );
      success = `Acesso a ${selectedTarget?.name ?? 'banco'} atualizado: ${accessLevelLabel(accessLevel).toLocaleLowerCase('pt-BR')}. Válido na próxima operação MCP.`;
      await loadGrants();
    } catch (cause) {
      actionError = cause instanceof Error ? cause.message : 'Não foi possível salvar a permissão.';
    } finally {
      saving = false;
    }
  }
  async function revoke(grant: DatabaseGrant, principalId: string): Promise<void> {
    actionError = undefined;
    success = undefined;
    saving = true;
    try {
      await apiFetch(`/v1/admin/principals/${principalId}/database-grants/${grant.database_id}`, {
        method: 'DELETE'
      });
      success = `Acesso a ${grant.database_name} revogado.`;
      dialog = { type: 'none' };
      await loadGrants();
    } catch (cause) {
      actionError =
        cause instanceof Error ? cause.message : 'Não foi possível revogar a permissão.';
    } finally {
      saving = false;
    }
  }
</script>

<svelte:head><title>Pessoas e acessos · SQL MCP Gateway</title></svelte:head>
<div class="page-heading">
  <div>
    <h1>Pessoas e acessos</h1>
    <p class="page-lede">A pessoa certa. O banco certo. Apenas a permissão necessária.</p>
  </div>
</div>
{#if loading}<div class="loading-panel" role="status">Carregando pessoas e permissões…</div>
{:else if session.currentPrincipal?.platform_role !== 'admin'}<div class="notice">
    A gestão de permissões é restrita a administradores. <a class="text-link" href="/"
      >Ver meus acessos</a
    >
  </div>
{:else if error}<div class="notice notice-error" role="alert">
    <p>{error}</p>
    <button class="button button-secondary" onclick={loadPage}>Tentar novamente</button>
  </div>
{:else if principals.length === 0}<section class="panel empty-state">
    <Icon name="users" size={34} />
    <h2>Convide a equipe a entrar</h2>
    <p>
      As pessoas aparecem aqui após o primeiro login com SSO. Depois você poderá conceder acesso por
      banco.
    </p>
  </section>
{:else}
  {#if success}<p class="notice notice-success" role="status">{success}</p>{/if}
  {#if actionError && dialog.type === 'none'}<p class="notice notice-error" role="alert">
      {actionError}
    </p>{/if}
  <section class="access-workspace">
    <aside class="panel access-sidebar">
      <h2>Escolha uma pessoa</h2>
      <label class="select-label"
        >Pessoa<select bind:value={selectedPrincipalId} onchange={changePrincipal} disabled={saving}
          >{#each principals as principal}<option value={principal.id}
              >{principal.display_name ?? principal.email ?? principal.subject}{!principal.enabled
                ? ' (desabilitado)'
                : ''}</option
            >{/each}</select
        ></label
      >{#if selectedPerson}<div class="person-profile">
          <div class="avatar avatar-large">
            {(selectedPerson.display_name ?? selectedPerson.email ?? selectedPerson.subject)
              .slice(0, 1)
              .toUpperCase()}
          </div>
          <h3>{selectedPerson.display_name ?? selectedPerson.email ?? selectedPerson.subject}</h3>
          {#if selectedPerson.email}<p>{selectedPerson.email}</p>{/if}<span class="badge"
            >{selectedPerson.platform_role === 'admin' ? 'Administrador' : 'Membro'}</span
          >{#if !selectedPerson.enabled}<p class="notice notice-warning">
              Esta identidade está desabilitada.
            </p>{/if}
        </div>{/if}
      <p class="microcopy">
        A conta é identificada pelo provedor de login e pelo identificador individual, não apenas
        pelo e-mail.
      </p>
    </aside>
    <div class="panel access-main">
      <div class="panel-heading">
        <div>
          <h2>Permissões de banco</h2>
          <p class="section-description">
            Acesso individual de {selectedPerson?.display_name ??
              selectedPerson?.email ??
              'esta pessoa'}.
          </p>
        </div>
        <span class="count-pill">{grants.length}</span>
      </div>
      {#if grantsLoading}<div class="loading-panel" role="status">Carregando permissões…</div>
      {:else if grantError}<div class="notice notice-error" role="alert">
          <p>{grantError}</p>
          <button class="button button-secondary" onclick={loadGrants}>Tentar novamente</button>
        </div>
      {:else if grants.length === 0}<div class="empty-state compact-empty">
          <Icon name="shield" size={28} />
          <h3>Nenhum acesso concedido</h3>
          <p>Use o formulário abaixo para liberar o primeiro banco a esta pessoa.</p>
        </div>
      {:else}<div class="grant-list">
          {#each grants as grant}<div class="grant-row">
              <span class="engine-mark"><Icon name="database" size={18} /></span>
              <div class="target-description">
                <strong>{grant.database_name}</strong><span
                  >{grant.granted_at
                    ? `Concedido em ${new Date(grant.granted_at).toLocaleDateString('pt-BR')}`
                    : 'Permissão individual'}</span
                >
              </div>
              <span class="badge" class:badge-write={grant.access_level === 'write'}
                >{accessLevelLabel(grant.access_level)}</span
              ><button
                class="text-button text-danger"
                disabled={saving}
                aria-label={`Revogar acesso a ${grant.database_name}`}
                onclick={() => {
                  actionError = undefined;
                  dialog = { type: 'revoke', grant, principalId: selectedPrincipalId };
                }}>Revogar</button
              >
            </div>{/each}
        </div>{/if}
      {#if targets.length === 0}<div class="notice notice-warning">
          <p>Cadastre ou ative um banco antes de conceder novos acessos.</p>
          <a class="text-link" href="/databases"
            >Ir para bancos de dados <Icon name="arrow" size={16} /></a
          >
        </div>
      {:else}<form class="grant-form" onsubmit={setGrant}>
          <h3>Conceder ou alterar acesso</h3>
          <fieldset disabled={saving || grantsLoading || !!grantError || !selectedPerson?.enabled}>
            <div class="form-row">
              <label
                >Banco<select bind:value={selectedDatabaseId} required
                  >{#each targets as target}<option value={target.id}>{target.name}</option
                    >{/each}</select
                ></label
              ><label
                >Permissão<select bind:value={accessLevel}
                  ><option value="read">Somente leitura</option><option value="write"
                    >Leitura + escrita</option
                  ></select
                ></label
              >
            </div>
            <div class="permission-explanation" class:permission-write={accessLevel === 'write'}>
              <Icon name="shield" size={19} />
              <p>
                {accessLevel === 'read'
                  ? 'Permite consultar dados. Não permite inserir, alterar ou excluir registros.'
                  : 'Permite SELECT, INSERT, UPDATE e DELETE. Não permite alterar a estrutura nem administrar o banco.'}
              </p>
            </div>
            {#if existingGrant}<p class="microcopy">
                Permissão atual: {accessLevelLabel(existingGrant.access_level)}. Salvar substitui
                esse nível.
              </p>{/if}<button
              class="button button-primary"
              type="submit"
              disabled={!selectedPrincipalId || !selectedDatabaseId}
              >{saving ? 'Salvando…' : 'Salvar permissão'}<Icon name="check" size={17} /></button
            >
          </fieldset>
        </form>{/if}
    </div>
  </section>
{/if}
{#if dialog.type === 'revoke'}
  {@const grant = dialog.grant}
  {@const principalId = dialog.principalId}
  <ConfirmDialog
    title={`Revogar acesso a ${grant.database_name}?`}
    description="A pessoa selecionada deixará de poder fazer novas operações MCP neste banco. Para liberar o acesso novamente, será necessário conceder uma nova permissão."
    action="Revogar acesso"
    busy={saving}
    error={actionError}
    oncancel={() => {
      dialog = { type: 'none' };
    }}
    onconfirm={() => {
      void revoke(grant, principalId);
    }}
  />
{/if}
