<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch, session, waitForSession } from '$lib/session.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import {
    claudeCodeCommand,
    endpointList,
    openCodeConfig,
    shellArgument,
    type ClientChoice,
    type McpServer
  } from '$lib/client-setup';
  import type { AuthConfig } from '$lib/types';
  import { mcpEndpoint, mcpServerName } from '$lib/workspaces';

  let config = $state<AuthConfig | undefined>();
  let loading = $state(true);
  let error = $state<string | undefined>();
  let client = $state<ClientChoice>({ type: 'opencode' });
  let includeOthers = $state(false);
  let directory = $derived(session.workspaces);
  let serverName = $derived(mcpServerName(directory?.current));
  let servers = $derived.by((): McpServer[] => {
    if (!config) {
      return [];
    }
    const own = { name: serverName, endpoint: config.mcp_endpoint };
    if (!includeOthers || !directory) {
      return [own];
    }
    return [
      own,
      ...directory.others.map((workspace) => ({
        name: mcpServerName(workspace),
        endpoint: mcpEndpoint(workspace)
      }))
    ];
  });
  let setupText = $derived(
    client.type === 'opencode'
      ? openCodeConfig(servers)
      : client.type === 'claude-code'
        ? claudeCodeCommand(servers)
        : endpointList(servers)
  );
  // The command line saves a workspace under its slug at the first login.
  let stdioLogin = $derived(
    config ? `sql-mcp-gateway login ${shellArgument(new URL(config.mcp_endpoint).origin)}` : ''
  );
  let stdioCommand = $derived(
    directory
      ? `sql-mcp-gateway stdio ${directory.current.slug}`
      : config
        ? `sql-mcp-gateway stdio ${shellArgument(new URL(config.mcp_endpoint).origin)}`
        : ''
  );
  let feedback = $state<{ type: 'none' } | { type: 'copied'; label: string } | { type: 'failed' }>({
    type: 'none'
  });

  onMount(() => {
    void loadConfig();
  });
  async function loadConfig(): Promise<void> {
    loading = true;
    error = undefined;
    await waitForSession();
    if (!session.currentPrincipal) {
      loading = false;
      return;
    }
    try {
      const response = await apiFetch('/v1/auth/config');
      config = await response.json();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Não foi possível carregar a configuração.';
    } finally {
      loading = false;
    }
  }
  async function copy(value: string, label: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(value);
      feedback = { type: 'copied', label };
    } catch {
      feedback = { type: 'failed' };
    }
  }
  function selectClient(choice: ClientChoice): void {
    client = choice;
    feedback = { type: 'none' };
  }
</script>

<svelte:head><title>Conectar MCP · SQL MCP Gateway</title></svelte:head>
<div class="page-heading">
  <div>
    <h1>Conecte seu assistente</h1>
    <p class="page-lede">
      Escolha o cliente, copie a configuração e entre com sua conta. Sem copiar tokens.
    </p>
  </div>
  <span class="badge badge-blue"><Icon name="shield" size={14} /> Login via OAuth</span>
</div>
{#if loading}<div class="loading-panel" role="status">Preparando sua configuração…</div>
{:else if error}<div class="notice notice-error" role="alert">
    <p>{error}</p>
    <button class="button button-secondary" onclick={loadConfig}>Tentar novamente</button>
  </div>
{:else if config}
  {@const connection = config}
  <div class="connect-layout">
    <section class="panel setup-panel" aria-labelledby="client-title">
      <div class="setup-step">
        <span class="step-marker">1</span>
        <div>
          <h2 id="client-title">Qual cliente você usa?</h2>
          <p>As instruções abaixo se adaptam à sua escolha.</p>
        </div>
      </div>
      <div class="client-picker" role="group" aria-label="Escolher cliente MCP">
        <button
          class:chosen={client.type === 'opencode'}
          aria-pressed={client.type === 'opencode'}
          onclick={() => selectClient({ type: 'opencode' })}
          ><span class="client-symbol" aria-hidden="true">&gt;_</span><strong>OpenCode</strong><span
            >Arquivo de configuração</span
          ></button
        ><button
          class:chosen={client.type === 'claude-code'}
          aria-pressed={client.type === 'claude-code'}
          onclick={() => selectClient({ type: 'claude-code' })}
          ><span class="client-symbol" aria-hidden="true">/</span><strong>Claude Code</strong><span
            >Comando no terminal</span
          ></button
        ><button
          class:chosen={client.type === 'other'}
          aria-pressed={client.type === 'other'}
          onclick={() => selectClient({ type: 'other' })}
          ><Icon name="plug" size={23} /><strong>Outro cliente</strong><span>HTTP com OAuth</span
          ></button
        >
      </div>
      {#if directory && directory.others.length > 0}
        <label class="scope-toggle">
          <input type="checkbox" bind:checked={includeOthers} />
          <span
            >Incluir também
            {directory.others.map((workspace) => workspace.name).join(', ')}
            <small>Cada workspace entra com nome próprio e pede login separado.</small></span
          >
        </label>
      {/if}
      <div class="setup-instructions" aria-live="polite">
        <div class="setup-step">
          <span class="step-marker">2</span>
          <div>
            <h2>
              {client.type === 'opencode'
                ? 'Adicione ao opencode.json'
                : client.type === 'claude-code'
                  ? 'Registre o servidor no terminal'
                  : 'Cadastre o endereço do servidor'}
            </h2>
            <p>
              {client.type === 'opencode'
                ? 'Inclua este servidor na configuração V2 existente, preservando os outros servidores.'
                : client.type === 'claude-code'
                  ? 'Execute o comando no projeto em que deseja usar o gateway.'
                  : 'Escolha o transporte HTTP e habilite a autenticação OAuth no seu cliente.'}
            </p>
          </div>
        </div>
        <div class="code-block">
          <div class="code-toolbar">
            <span
              >{client.type === 'opencode'
                ? 'opencode.json'
                : client.type === 'claude-code'
                  ? 'Terminal'
                  : 'Endpoint MCP'}</span
            ><button class="button button-code" onclick={() => copy(setupText, 'Configuração')}
              ><Icon
                name={feedback.type === 'copied' && feedback.label === 'Configuração'
                  ? 'check'
                  : 'copy'}
                size={16}
              />{feedback.type === 'copied' && feedback.label === 'Configuração'
                ? 'Copiado'
                : 'Copiar'}</button
            >
          </div>
          <pre><code>{setupText}</code></pre>
        </div>
        {#if client.type === 'other'}<dl class="connection-details">
            <div>
              <dt>Autenticação</dt>
              <dd>OAuth 2.1 com PKCE S256 e registro dinâmico de clientes</dd>
            </div>
            <div>
              <dt>Descoberta OAuth</dt>
              <dd><code>{connection.oauth_authorization_metadata_url}</code></dd>
            </div>
            <div>
              <dt>Escopo</dt>
              <dd><code>mcp</code></dd>
            </div>
          </dl>{/if}
        <div class="setup-step">
          <span class="step-marker">3</span>
          <div>
            <h2>Entre e faça sua primeira consulta</h2>
            <p>
              {client.type === 'opencode'
                ? `Abra /mcps no OpenCode e entre no servidor ${serverName}.`
                : client.type === 'claude-code'
                  ? `Abra /mcp no Claude Code e conclua o login do servidor ${serverName} no navegador.`
                  : 'Inicie a conexão no cliente e conclua o login no navegador.'} Use a mesma conta deste
              painel.
            </p>
          </div>
        </div>
        <div class="first-query">
          <span>Experimente pedir ao assistente</span>
          <p>“Liste os bancos de dados aos quais tenho acesso.”</p>
        </div>
      </div>
      <div class="copy-feedback" class:copy-failed={feedback.type === 'failed'} role="status">
        {#if feedback.type === 'copied'}<Icon name="check" size={16} />
          Copiado para a área de transferência: {feedback.label.toLocaleLowerCase(
            'pt-BR'
          )}.{:else if feedback.type === 'failed'}Não foi possível copiar. Selecione o texto do
          bloco e copie manualmente.{/if}
      </div>
    </section>
    <aside class="connect-help">
      <section class="context-panel">
        <Icon name="database" size={25} />
        <h2>O que você poderá acessar?</h2>
        <p>
          O cliente usa as permissões da sua identidade. Conectar não concede acesso a novos bancos.
        </p>
        <a class="text-link" href="/">Ver meus acessos <Icon name="arrow" size={16} /></a>
      </section>
      <section class="help-section">
        <h3>Um banco não apareceu?</h3>
        <p>Peça ao administrador que confira a concessão para a mesma conta usada no cliente.</p>
        <h3>O cliente não abre o login?</h3>
        <p>
          Confirme que ele aceita MCP remoto via HTTP e OAuth. Para clientes sem esse suporte, use a
          alternativa stdio abaixo.
        </p>
      </section>
    </aside>
  </div>
  <details class="panel stdio-details">
    <summary>Alternativa: conectar por subprocesso (stdio)</summary>
    <div class="details-body">
      <p>
        Instale o binário <code>sql-mcp-gateway</code>. Faça login uma vez e configure o cliente
        para executar o segundo comando.
      </p>
      <div class="code-block">
        <div class="code-toolbar">
          <span>1. Login no navegador</span><button
            class="button button-code"
            onclick={() => copy(stdioLogin, 'Comando de login')}
            ><Icon
              name={feedback.type === 'copied' && feedback.label === 'Comando de login'
                ? 'check'
                : 'copy'}
              size={16}
            />
            {feedback.type === 'copied' && feedback.label === 'Comando de login'
              ? 'Copiado'
              : 'Copiar login'}</button
          >
        </div>
        <pre><code>{stdioLogin}</code></pre>
      </div>
      <div class="code-block">
        <div class="code-toolbar">
          <span>2. Comando do subprocesso</span><button
            class="button button-code"
            onclick={() => copy(stdioCommand, 'Comando stdio')}
            ><Icon
              name={feedback.type === 'copied' && feedback.label === 'Comando stdio'
                ? 'check'
                : 'copy'}
              size={16}
            />
            {feedback.type === 'copied' && feedback.label === 'Comando stdio'
              ? 'Copiado'
              : 'Copiar stdio'}</button
          >
        </div>
        <pre><code>{stdioCommand}</code></pre>
      </div>
      <p class="microcopy">
        {#if directory && directory.others.length > 0}O login oferece adicionar os outros workspaces
          de uma vez. Depois, <code>sql-mcp-gateway workspaces</code> lista todos e
          <code>sql-mcp-gateway login</code> abre um seletor.{/if}
        Os tokens rotativos ficam no keyring do sistema, nunca em arquivo de configuração. Sem keyring,
        o login falha. O subprocesso conecta ao gateway remoto; não exige Vault nem banco local.
      </p>
    </div>
  </details>
{/if}
