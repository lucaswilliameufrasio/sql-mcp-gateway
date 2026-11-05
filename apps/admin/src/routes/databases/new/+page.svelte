<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch, session, waitForSession } from '$lib/session.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import type { DatabaseEngine } from '$lib/types';

  let loading = $state(true);
  let saving = $state(false);
  let error = $state<string | undefined>();
  let success = $state<string | undefined>();
  let name = $state('');
  let connectionMode = $state<'fields' | 'url'>('fields');
  let databaseUrl = $state('');
  let engine = $state<DatabaseEngine>('postgresql');
  let host = $state('');
  let port = $state(5432);
  let databaseName = $state('');
  let schemaName = $state('public');
  let tlsMode = $state<'verify_full' | 'disable'>('verify_full');
  let rootUsername = $state('');
  let rootPassword = $state('');
  let tlsCaPem = $state('');

  onMount(() => {
    void waitForSession().then(() => {
      loading = false;
    });
  });

  function changeEngine(event: Event): void {
    if (!(event.currentTarget instanceof HTMLSelectElement)) {
      return;
    }
    engine = event.currentTarget.value === 'mysql' ? 'mysql' : 'postgresql';
    port = engine === 'postgresql' ? 5432 : 3306;
    if (engine === 'mysql' && schemaName === 'public') {
      schemaName = databaseName;
    }
    if (engine === 'postgresql' && !schemaName) {
      schemaName = 'public';
    }
  }

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    error = undefined;
    success = undefined;
    saving = true;
    try {
      const body: Record<string, unknown> = { name: name.trim() };
      body.tls_mode = tlsMode;
      if (connectionMode === 'url') {
        body.database_url = databaseUrl.trim();
      } else {
        body.engine = engine;
        body.host = host.trim();
        body.port = Number(port);
        body.database_name = databaseName.trim();
        body.schema_name = schemaName.trim() || undefined;
        body.root_username = rootUsername.trim();
        body.root_password = rootPassword;
      }
      if (tlsCaPem.trim()) {
        body.tls_ca_pem = tlsCaPem.trim();
      }

      await apiFetch('/v1/admin/database-targets', {
        method: 'POST',
        body: JSON.stringify(body)
      });
      name = '';
      databaseUrl = '';
      host = '';
      databaseName = '';
      schemaName = engine === 'postgresql' ? 'public' : '';
      rootUsername = '';
      rootPassword = '';
      tlsCaPem = '';
      tlsMode = 'verify_full';
      success = 'Banco registrado. A senha foi enviada ao Vault e não é mantida pelo painel.';
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Não foi possível registrar o banco.';
    } finally {
      saving = false;
    }
  }
</script>

<svelte:head><title>Cadastrar banco · SQL MCP Gateway</title></svelte:head>

<a class="back-link" href="/databases">Voltar para bancos de dados</a>
<div class="page-heading">
  <div>
    <h1>Cadastrar banco</h1>
    <p class="page-lede">Configure a conexão. Depois, escolha quem pode acessá-la.</p>
  </div>
</div>

{#if loading}<div class="loading-panel" role="status">Preparando cadastro…</div>
{:else if session.currentPrincipal?.platform_role !== 'admin'}<div class="notice">
    Somente administradores podem cadastrar bancos. <a class="text-link" href="/connect"
      >Conectar meu cliente MCP</a
    >
  </div>
{:else if success}<section class="panel registration-success" role="status">
    <div class="success-mark"><Icon name="check" size={28} /></div>
    <h2>Banco cadastrado</h2>
    <p>{success}</p>
    <p>O próximo passo é conceder acesso às pessoas da equipe.</p>
    <div class="inline-actions">
      <a class="button button-primary" href="/access"
        >Conceder acesso <Icon name="arrow" size={18} /></a
      ><a class="button button-secondary" href="/databases">Ver bancos</a>
    </div>
  </section>
{:else}
  <div class="registration-layout">
    <section class="panel form-panel">
      <div class="panel-heading">
        <div>
          <h2>Dados da conexão</h2>
          <p class="section-description">Todos os campos são obrigatórios, exceto os indicados.</p>
        </div>
      </div>
      <form onsubmit={submit}>
        <fieldset disabled={saving}>
          <label
            >Nome de exibição<input
              bind:value={name}
              required
              maxlength="80"
              placeholder="ex.: warehouse-prod"
            /></label
          >
          <label
            >Formato da conexão
            <select bind:value={connectionMode}>
              <option value="fields">Informar os campos</option>
              <option value="url">Usar database URL</option>
            </select>
          </label>
          <label
            >Segurança da conexão
            <select
              bind:value={tlsMode}
              onchange={() => {
                if (tlsMode === 'disable') {
                  tlsCaPem = '';
                }
              }}
            >
              <option value="verify_full">TLS verificado (recomendado)</option>
              <option value="disable">Sem TLS (não criptografado)</option>
            </select>
            {#if tlsMode === 'disable'}
              <small role="alert" class="notice notice-warning"
                >A conexão com o banco não será criptografada, incluindo as credenciais. Use somente
                em rede confiável.</small
              >
            {/if}
          </label>
          {#if connectionMode === 'url'}
            <label
              >Database URL<input
                bind:value={databaseUrl}
                required
                type="password"
                autocomplete="off"
                spellcheck="false"
                placeholder="postgresql://usuario:senha@db.example.test:5432/analytics"
              />
              <small
                >A URL é usada apenas para configurar o Vault e não é armazenada. Parâmetros de
                query não são aceitos; a opção de segurança acima define o TLS.</small
              >
            </label>
          {:else}
            <div class="form-row">
              <label
                >Tipo de banco
                <select value={engine} onchange={changeEngine}>
                  <option value="postgresql">PostgreSQL</option><option value="mysql">MySQL</option>
                </select>
              </label>
              <label
                >Porta<input bind:value={port} type="number" min="1" max="65535" required /></label
              >
            </div>
            <label
              >Host privado<input
                bind:value={host}
                required
                maxlength="253"
                placeholder="db.internal.example"
                autocomplete="off"
              /></label
            >
            <div class="form-row">
              <label
                >Banco<input
                  bind:value={databaseName}
                  required
                  maxlength="63"
                  placeholder="analytics"
                /></label
              >
              <label
                >Schema (opcional)<input
                  bind:value={schemaName}
                  maxlength="63"
                  placeholder="public"
                /></label
              >
            </div>
            <div class="form-divider">
              <h3>Credenciais de provisionamento</h3>
              <p>O Vault usa este acesso para criar credenciais temporárias para os usuários.</p>
            </div>
            <label
              >Usuário administrativo do banco<input
                bind:value={rootUsername}
                required
                maxlength="128"
                autocomplete="username"
              /></label
            >
            <label
              >Senha administrativa
              <input
                bind:value={rootPassword}
                type="password"
                required
                autocomplete="new-password"
              />
              <small>Usada apenas para configurar o Vault; não é armazenada no gateway.</small>
            </label>
          {/if}
          {#if tlsMode === 'verify_full'}
            <details class="advanced-fields">
              <summary>Certificado de CA privada (opcional)</summary>
              <label
                >Certificado CA PEM
                <textarea
                  bind:value={tlsCaPem}
                  rows="5"
                  spellcheck="false"
                  placeholder="-----BEGIN CERTIFICATE-----"
                ></textarea>
                <small
                  >Use quando o banco for assinado por uma CA privada. A validação do hostname
                  permanece ativa.</small
                >
              </label>
            </details>
          {/if}
        </fieldset>
        {#if error}<p class="notice notice-error" role="alert">
            {error} Confira os dados da conexão e tente novamente.
          </p>{/if}
        <div class="form-actions">
          <a
            class="button button-secondary"
            href="/databases"
            aria-disabled={saving}
            onclick={(event) => {
              if (saving) {
                event.preventDefault();
              }
            }}>Cancelar</a
          >
          <button class="button button-primary button-wide" type="submit" disabled={saving}>
            {saving ? 'Validando e cadastrando…' : 'Validar e cadastrar banco'}<Icon
              name="check"
              size={18}
            />
          </button>
        </div>
      </form>
    </section>
    <aside class="registration-help">
      <Icon name="shield" size={26} />
      <h2>O que acontece ao cadastrar?</h2>
      <ol>
        <li>
          <strong>A conexão é validada</strong>
          <p>
            O gateway verifica se o Vault consegue acessar o banco com a configuração informada.
          </p>
        </li>
        <li>
          <strong>As credenciais ficam no Vault</strong>
          <p>A senha administrativa não fica armazenada neste painel.</p>
        </li>
        <li>
          <strong>Você libera os acessos</strong>
          <p>
            Cadastrar o banco não concede permissão aos membros. Faça isso em Pessoas e acessos.
          </p>
        </li>
      </ol>
      <p class="microcopy">
        TLS verificado é a opção recomendada. Desabilite somente quando o banco e a rede exigirem
        essa escolha.
      </p>
    </aside>
  </div>
{/if}
