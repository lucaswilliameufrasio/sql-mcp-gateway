<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch, session, waitForSession } from '$lib/session.svelte';
  import { accessLevelLabel } from '$lib/presentation';
  import Icon from '$lib/components/Icon.svelte';
  import UsageMetrics from '$lib/metrics/UsageMetrics.svelte';
  import type { DatabaseTarget, MeResponse } from '$lib/types';

  let targets = $state<DatabaseTarget[]>([]);
  let grants = $state<MeResponse['database_grants']>([]);
  let loading = $state(true);
  let error = $state<string | undefined>();
  let isAdmin = $derived(session.currentPrincipal?.platform_role === 'admin');
  let enabledTargets = $derived(targets.filter((target) => target.enabled));

  onMount(() => {
    void loadOverview();
  });
  async function loadOverview(): Promise<void> {
    loading = true;
    error = undefined;
    await waitForSession();
    if (!session.currentPrincipal) {
      loading = false;
      return;
    }
    try {
      if (isAdmin) {
        const targetsResponse = await apiFetch('/v1/admin/database-targets');
        targets = await targetsResponse.json();
      } else {
        const response = await apiFetch('/v1/me');
        const me: MeResponse = await response.json();
        grants = me.database_grants;
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
    <h1>Seu workspace de dados</h1>
    <p class="page-lede">
      {isAdmin
        ? 'Do cadastro à primeira consulta: gerencie cada etapa do acesso.'
        : 'Confira seus acessos e leve os dados para o seu assistente.'}
    </p>
  </div>
  <a class="button button-primary" href={isAdmin ? '/databases/new' : '/connect'}
    ><Icon name={isAdmin ? 'plus' : 'plug'} size={18} />{isAdmin
      ? 'Cadastrar banco'
      : 'Conectar meu cliente'}</a
  >
</div>

<UsageMetrics />

{#if loading}<div class="loading-panel" role="status">Carregando seu workspace…</div>
{:else if error}<div class="notice notice-error" role="alert">
    <p>{error}</p>
    <button class="button button-secondary" onclick={loadOverview}>Tentar novamente</button>
  </div>
{:else}
  {#if isAdmin && targets.length === 0}
    <section class="journey-panel" aria-labelledby="start-title">
      <div class="section-intro">
        <h2 id="start-title">Um caminho simples até os dados</h2>
        <p>Configure a infraestrutura uma vez. Conceda acesso conforme a equipe precisa.</p>
      </div>
      <div class="journey-grid">
        <a href="/databases/new"
          ><span class="step-marker">1</span><strong>Cadastre um banco</strong>
          <p>PostgreSQL ou MySQL, com a política de TLS explícita.</p>
          <span class="text-link">Configurar conexão <Icon name="arrow" size={16} /></span></a
        ><a href="/access"
          ><span class="step-marker">2</span><strong>Libere os acessos</strong>
          <p>Escolha a pessoa, o banco e o nível de permissão.</p>
          <span class="text-link">Gerenciar permissões <Icon name="arrow" size={16} /></span></a
        ><a href="/connect"
          ><span class="step-marker">3</span><strong>Conecte o cliente MCP</strong>
          <p>Copie a configuração e entre com sua identidade.</p>
          <span class="text-link">Configurar cliente <Icon name="arrow" size={16} /></span></a
        >
      </div>
    </section>
  {:else if !isAdmin && grants.length === 0}
    <section class="member-welcome">
      <div class="welcome-icon"><Icon name="plug" size={32} /></div>
      <div>
        <h2>Seu assistente, com o acesso que é seu.</h2>
        <p>Conecte com OAuth. Você não precisa copiar tokens nem conhecer a senha dos bancos.</p>
      </div>
      <a class="button button-secondary" href="/connect"
        >Configurar MCP <Icon name="arrow" size={17} /></a
      >
    </section>
  {/if}
  <section class="content-grid">
    <article class="panel database-panel">
      <div class="panel-heading">
        <div>
          <h2>{isAdmin ? 'Bancos disponíveis' : 'Seus bancos'}</h2>
          <p class="section-description">
            {isAdmin
              ? 'Conexões habilitadas no gateway.'
              : 'Permissões concedidas à sua identidade, banco por banco.'}
          </p>
        </div>
        {#if isAdmin}<a class="text-link" href="/databases"
            >Ver catálogo <Icon name="arrow" size={16} /></a
          >{:else}<span class="count-pill">{grants.length}</span>{/if}
      </div>
      {#if isAdmin}
        {#if enabledTargets.length === 0}<div class="empty-state">
            <Icon name="database" size={32} />
            <h3>Comece pelo primeiro banco</h3>
            <p>Cadastre uma conexão para que a equipe possa consultar dados pelo MCP.</p>
            <a class="button button-secondary" href="/databases/new">Cadastrar banco</a>
          </div>
        {:else}<div class="target-list">
            {#each enabledTargets.slice(0, 4) as target}<a class="target-row" href="/databases"
                ><span class="engine-mark" class:engine-mysql={target.engine === 'mysql'}
                  >{target.engine === 'mysql' ? 'My' : 'Pg'}</span
                >
                <div class="target-description">
                  <strong>{target.name}</strong><span
                    >{target.database_name} · {target.engine === 'mysql'
                      ? 'MySQL'
                      : 'PostgreSQL'}</span
                  >
                </div>
                <span class="badge badge-live">Ativo</span></a
              >{/each}
          </div>{/if}
      {:else if grants.length === 0}<div class="empty-state">
          <Icon name="shield" size={32} />
          <h3>Seu primeiro acesso começa com a equipe</h3>
          <p>
            Peça ao administrador acesso a um banco. Enquanto isso, você já pode configurar seu
            cliente.
          </p>
          <a class="button button-secondary" href="/connect">Configurar cliente MCP</a>
        </div>
      {:else}<div class="target-list">
          {#each grants as grant}<div class="target-row">
              <span class="engine-mark"><Icon name="database" size={18} /></span>
              <div class="target-description">
                <strong>{grant.database_name}</strong><span>Permissão individual</span>
              </div>
              <span class="badge" class:badge-write={grant.access_level === 'write'}
                >{accessLevelLabel(grant.access_level)}</span
              >
            </div>{/each}
        </div>{/if}
    </article>
    <aside class="context-panel">
      <Icon name="shield" size={24} />
      <h2>
        {isAdmin
          ? 'Você controla o acesso. O gateway cuida da conexão.'
          : 'Suas permissões, também no assistente.'}
      </h2>
      <p>
        O login identifica a pessoa. A concessão define quais bancos ela pode usar. As credenciais
        do banco são temporárias.
      </p>
      {#if isAdmin}<a class="text-link" href="/audit"
          >Consultar auditoria <Icon name="arrow" size={16} /></a
        >{:else}<p class="microcopy">
          Precisa de outro banco ou de permissão de escrita? Fale com o administrador do gateway.
        </p>{/if}
    </aside>
  </section>
{/if}
