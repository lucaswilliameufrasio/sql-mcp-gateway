<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch, session, waitForSession } from '$lib/session.svelte';
  import type { AuthConfig } from '$lib/types';

  type ConnectView = { kind: 'idle' } | { kind: 'copied'; client: string };

  let config = $state<AuthConfig | undefined>();
  let loading = $state(true);
  let error = $state<string | undefined>();
  let view = $state<ConnectView>({ kind: 'idle' });

  onMount(() => {
    void loadConfig();
  });

  async function loadConfig(): Promise<void> {
    await waitForSession();
    if (!session.currentPrincipal) {
      loading = false;
      return;
    }

    try {
      const response = await apiFetch('/v1/auth/config');
      config = (await response.json()) as AuthConfig;
    } catch (cause) {
      error =
        cause instanceof Error ? cause.message : 'Não foi possível carregar o endereço do gateway.';
    } finally {
      loading = false;
    }
  }

  function openCodeSnippet(): string {
    if (!config) return '';
    return JSON.stringify(
      {
        $schema: 'https://opencode.ai/config.json',
        mcp: {
          servers: {
            'sql-mcp-gateway': {
              type: 'remote',
              url: config.mcp_endpoint
            }
          }
        }
      },
      null,
      2
    );
  }

  function claudeCodeCommand(): string {
    return `claude mcp add --transport http sql-mcp-gateway ${config?.mcp_endpoint ?? ''}`;
  }

  async function copy(value: string, client: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(value);
      view = { kind: 'copied', client };
      window.setTimeout(() => {
        if (view.kind === 'copied' && view.client === client) view = { kind: 'idle' };
      }, 1800);
    } catch {
      error = 'Não foi possível copiar. Selecione e copie o texto manualmente.';
    }
  }
</script>

<svelte:head>
  <title>Conectar MCP · SQL MCP Gateway</title>
</svelte:head>

<div class="page-heading">
  <div>
    <p class="eyebrow">ACESSO PESSOAL</p>
    <h1>Conectar MCP<span class="heading-period">.</span></h1>
    <p class="page-lede">
      Use login OAuth no cliente. Nenhum token precisa ser copiado para a configuração.
    </p>
  </div>
</div>

{#if loading}
  <p class="notice" aria-live="polite">Carregando a configuração do gateway…</p>
{:else if error}
  <p class="notice notice-error" role="alert">{error}</p>
{:else if config}
  <section class="connect-intro">
    <div>
      <p class="eyebrow">ENDPOINT REMOTO · HTTP</p>
      <h2>Recomendado para clientes com OAuth MCP</h2>
      <p>
        O cliente abre o provedor de identidade para você entrar. O gateway recebe tokens próprios e
        continua aplicando suas concessões por banco.
      </p>
    </div>
    <div class="connect-endpoint">
      <span>ENDPOINT MCP</span>
      <code>{config.mcp_endpoint}</code>
    </div>
  </section>

  <div class="connect-grid">
    <article class="connect-card">
      <div class="connect-card-top">
        <span class="connect-index">01</span><span class="eyebrow">OPENCODE</span>
      </div>
      <h2>Adicionar servidor remoto</h2>
      <p>
        Adicione este bloco ao seu <code>opencode.json</code> V2. OAuth remoto é ativado por padrão;
        na primeira conexão, use <code>/mcps</code> para entrar.
      </p>
      <pre><code>{openCodeSnippet()}</code></pre>
      <button class="button button-secondary" onclick={() => copy(openCodeSnippet(), 'OpenCode')}>
        {view.kind === 'copied' && view.client === 'OpenCode' ? 'Copiado' : 'Copiar configuração'}
      </button>
    </article>

    <article class="connect-card">
      <div class="connect-card-top">
        <span class="connect-index">02</span><span class="eyebrow">CLAUDE CODE</span>
      </div>
      <h2>Registrar via CLI</h2>
      <p>
        O transporte HTTP permite que Claude Code descubra e complete o fluxo OAuth no navegador.
      </p>
      <pre><code>{claudeCodeCommand()}</code></pre>
      <button
        class="button button-secondary"
        onclick={() => copy(claudeCodeCommand(), 'Claude Code')}
      >
        {view.kind === 'copied' && view.client === 'Claude Code' ? 'Copiado' : 'Copiar comando'}
      </button>
    </article>
  </div>

  <section class="connect-generic">
    <div>
      <p class="eyebrow">OUTROS CLIENTES COMPATÍVEIS</p>
      <h2>Configuração HTTP + OAuth</h2>
      <p>
        Cadastre o endpoint MCP abaixo no cliente, escolha OAuth 2.1 com PKCE S256 e permita a
        descoberta do servidor de autorização e o registro dinâmico de clientes.
      </p>
    </div>
    <dl>
      <div>
        <dt>Endpoint MCP</dt>
        <dd><code>{config.mcp_endpoint}</code></dd>
      </div>
      <div>
        <dt>Descoberta OAuth</dt>
        <dd><code>{config.oauth_authorization_metadata_url}</code></dd>
      </div>
      <div>
        <dt>Escopo</dt>
        <dd><code>mcp</code></dd>
      </div>
    </dl>
  </section>

  <section class="connect-note">
    <p class="eyebrow">MODO STDIO</p>
    <p>
      Para usar subprocesso sem colar tokens, execute
      <code>sql-mcp-gateway --login {config.mcp_endpoint}</code> uma vez. O navegador conclui OAuth
      e os tokens rotativos são guardados no keyring do sistema; depois configure o cliente para
      executar
      <code>sql-mcp-gateway --stdio {config.mcp_endpoint}</code>. O subprocesso conecta ao gateway
      remoto; não precisa de banco de controle, OIDC ou Vault locais. Se não houver keyring
      disponível, o login falha sem gravar credenciais em arquivo. <code>SQL_MCP_ACCESS_TOKEN</code> continua
      aceito apenas como compatibilidade legada.
    </p>
  </section>
  <p class="connect-footnote">
    A autenticação identifica você; não concede acesso a bancos. Peça a um administrador que conceda
    o acesso necessário.
  </p>
{/if}
