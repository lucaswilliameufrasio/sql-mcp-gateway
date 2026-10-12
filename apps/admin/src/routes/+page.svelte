<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch, session, waitForSession } from '$lib/session.svelte';
  import { accessLevelLabel } from '$lib/presentation';
  import type { DatabaseTarget, MeResponse, Principal } from '$lib/types';

  let databaseCount = $state(0);
  let memberCount = $state(0);
  let disabledCount = $state(0);
  let recentTargets = $state<DatabaseTarget[]>([]);
  let grants = $state<MeResponse['database_grants']>([]);
  let loading = $state(true);
  let error = $state<string | undefined>();

  onMount(() => {
    void loadOverview();
  });

  async function loadOverview(): Promise<void> {
    await waitForSession();
    if (!session.currentPrincipal) {
      loading = false;
      return;
    }

    try {
      if (session.currentPrincipal.platform_role === 'admin') {
        const [targetsResponse, usersResponse] = await Promise.all([
          apiFetch('/v1/admin/database-targets'),
          apiFetch('/v1/admin/principals')
        ]);
        const targets = (await targetsResponse.json()) as DatabaseTarget[];
        const users = (await usersResponse.json()) as Principal[];
        databaseCount = targets.filter((target) => target.enabled).length;
        disabledCount = targets.filter((target) => !target.enabled).length;
        memberCount = users.filter(
          (user) => user.platform_role === 'member' && user.enabled
        ).length;
        recentTargets = targets.filter((target) => target.enabled).slice(0, 4);
      } else {
        const response = await apiFetch('/v1/me');
        const me = (await response.json()) as MeResponse;
        grants = me.database_grants;
        databaseCount = grants.length;
      }
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Não foi possível carregar o resumo.';
    } finally {
      loading = false;
    }
  }
</script>

<svelte:head><title>Visão geral · SQL MCP Gateway</title></svelte:head>

<div class="page-heading">
  <div>
    <p class="eyebrow">SEU PLANO DE CONTROLE</p>
    <h1>Visão geral<span class="heading-period">.</span></h1>
    <p class="page-lede">
      {session.currentPrincipal?.platform_role === 'admin'
        ? 'Acompanhe bancos, identidades e o perímetro de acesso.'
        : 'Veja os bancos liberados para a sua identidade.'}
    </p>
  </div>
  {#if session.currentPrincipal?.platform_role === 'admin'}
    <a class="button button-primary" href="/databases"
      >Cadastrar banco <span aria-hidden="true">＋</span></a
    >
  {/if}
</div>

{#if error}
  <p class="notice notice-error" role="alert">{error}</p>
{/if}

{#if loading}
  <div class="loading-panel" aria-live="polite">Carregando o estado do workspace…</div>
{:else}
  <section class="metric-grid" aria-label="Resumo do workspace">
    <article class="metric-card metric-featured">
      <div class="metric-topline"><span>ALVOS ATIVOS</span><span class="metric-glyph">▤</span></div>
      <div class="metric-value">{databaseCount.toString().padStart(2, '0')}</div>
      <p>
        {session.currentPrincipal?.platform_role === 'admin'
          ? 'Bancos registrados e disponíveis'
          : 'Bancos concedidos à sua identidade'}
      </p>
    </article>
    {#if session.currentPrincipal?.platform_role === 'admin'}
      <article class="metric-card">
        <div class="metric-topline">
          <span>IDENTIDADES</span><span class="metric-glyph">⊙</span>
        </div>
        <div class="metric-value">{memberCount.toString().padStart(2, '0')}</div>
        <p>Membros que já entraram com SSO</p>
      </article>
      <article class="metric-card">
        <div class="metric-topline"><span>EM PAUSA</span><span class="metric-glyph">Ⅱ</span></div>
        <div class="metric-value">{disabledCount.toString().padStart(2, '0')}</div>
        <p>Alvos desabilitados, sem novas sessões</p>
      </article>
    {:else}
      <article class="metric-card">
        <div class="metric-topline">
          <span>NÍVEL DE ACESSO</span><span class="metric-glyph">⌑</span>
        </div>
        <div class="metric-value metric-value-word">
          {grants.some((grant) => grant.access_level === 'write')
            ? accessLevelLabel('write')
            : 'Somente leitura'}
        </div>
        <p>As permissões também são aplicadas pelo próprio banco</p>
      </article>
      <article class="metric-card metric-card-note">
        <div class="metric-topline"><span>SESSÃO</span><span class="status-dot"></span></div>
        <div class="metric-note-title">Identidade verificada</div>
        <p>Seu token é validado pelo provedor OIDC a cada operação.</p>
      </article>
    {/if}
  </section>

  <section class="content-grid">
    <article class="panel database-panel">
      <div class="panel-heading">
        <div>
          <p class="eyebrow">CONEXÕES AUTORIZADAS</p>
          <h2>
            {session.currentPrincipal?.platform_role === 'admin'
              ? 'Bancos no gateway'
              : 'Seus bancos'}
          </h2>
        </div>
        {#if session.currentPrincipal?.platform_role === 'admin'}
          <a class="text-link" href="/databases">Ver todos <span aria-hidden="true">→</span></a>
        {/if}
      </div>
      {#if session.currentPrincipal?.platform_role === 'admin'}
        {#if recentTargets.length === 0}
          <div class="empty-state">
            <div class="empty-mark">▤</div>
            <strong>Nenhum banco cadastrado</strong>
            <p>Adicione um alvo PostgreSQL ou MySQL para iniciar.</p>
            <a class="text-link" href="/databases">Cadastrar o primeiro banco →</a>
          </div>
        {:else}
          <div class="target-list">
            {#each recentTargets as target}
              <div class="target-row">
                <span class="engine-mark" class:engine-mysql={target.engine === 'mysql'}
                  >{target.engine === 'mysql' ? 'MY' : 'PG'}</span
                >
                <div class="target-description">
                  <strong>{target.name}</strong><span
                    >{target.host}:{target.port} · {target.database_name}</span
                  >
                </div>
                <span class="badge badge-live">ATIVO</span>
              </div>
            {/each}
          </div>
        {/if}
      {:else if grants.length === 0}
        <div class="empty-state">
          <div class="empty-mark">⌑</div>
          <strong>Sem acessos atribuídos</strong>
          <p>Peça a um administrador do gateway para conceder acesso.</p>
        </div>
      {:else}
        <div class="target-list">
          {#each grants as grant}
            <div class="target-row">
              <span class="engine-mark">DB</span>
              <div class="target-description">
                <strong>{grant.database_name}</strong><span>Concessão individual</span>
              </div>
              <span class="badge" class:badge-write={grant.access_level === 'write'}
                >{accessLevelLabel(grant.access_level)}</span
              >
            </div>
          {/each}
        </div>
      {/if}
    </article>

    <aside class="panel perimeter-panel">
      <p class="eyebrow">MODELO DE SEGURANÇA</p>
      <h2>Sem atalho<br />para o banco.</h2>
      <div class="perimeter-flow">
        <div><span class="flow-number">01</span><span>OIDC valida a identidade</span></div>
        <div><span class="flow-number">02</span><span>Gateway verifica a concessão</span></div>
        <div><span class="flow-number">03</span><span>Vault emite credencial efêmera</span></div>
      </div>
      <p class="perimeter-foot">
        Cada lease expira em 15 minutos. O acesso final nunca excede os privilégios do banco.
      </p>
    </aside>
  </section>
{/if}
