<script lang="ts">
  import { onMount } from 'svelte';
  import { apiFetch, waitForSession } from '$lib/session.svelte';
  import type { DatabaseEngine, DatabaseTarget } from '$lib/types';

  let targets = $state<DatabaseTarget[]>([]);
  let loading = $state(true);
  let saving = $state(false);
  let error = $state<string | undefined>();
  let success = $state<string | undefined>();
  let name = $state('');
  let engine = $state<DatabaseEngine>('postgresql');
  let host = $state('');
  let port = $state(5432);
  let databaseName = $state('');
  let schemaName = $state('public');
  let rootUsername = $state('');
  let rootPassword = $state('');
  let tlsCaPem = $state('');

  onMount(() => {
    void loadTargets();
  });

  async function loadTargets(): Promise<void> {
    await waitForSession();
    try {
      const response = await apiFetch('/v1/admin/database-targets');
      targets = (await response.json()) as DatabaseTarget[];
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Não foi possível carregar os bancos.';
    } finally {
      loading = false;
    }
  }

  function changeEngine(event: Event): void {
    const select = event.currentTarget as HTMLSelectElement;
    engine = select.value as DatabaseEngine;
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
      const body: Record<string, unknown> = {
        name: name.trim(),
        engine,
        host: host.trim(),
        port: Number(port),
        database_name: databaseName.trim(),
        schema_name: schemaName.trim() || undefined,
        root_username: rootUsername.trim(),
        root_password: rootPassword
      };
      if (tlsCaPem.trim()) {
        body.tls_ca_pem = tlsCaPem.trim();
      }

      await apiFetch('/v1/admin/database-targets', {
        method: 'POST',
        body: JSON.stringify(body)
      });
      name = '';
      host = '';
      databaseName = '';
      schemaName = engine === 'postgresql' ? 'public' : '';
      rootUsername = '';
      rootPassword = '';
      tlsCaPem = '';
      success = 'Banco registrado. A senha foi enviada ao Vault e não é mantida pelo painel.';
      await loadTargets();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Não foi possível registrar o banco.';
    } finally {
      saving = false;
    }
  }

  async function setEnabled(target: DatabaseTarget): Promise<void> {
    error = undefined;
    try {
      await apiFetch(`/v1/admin/database-targets/${target.id}/enabled`, {
        method: 'PUT',
        body: JSON.stringify({ enabled: !target.enabled })
      });
      await loadTargets();
    } catch (cause) {
      error =
        cause instanceof Error ? cause.message : 'Não foi possível alterar o estado do banco.';
    }
  }
</script>

<svelte:head><title>Bancos de dados · SQL MCP Gateway</title></svelte:head>

<div class="page-heading">
  <div>
    <p class="eyebrow">INFRAESTRUTURA AUTORIZADA</p>
    <h1>Bancos de dados<span class="heading-period">.</span></h1>
    <p class="page-lede">
      Cadastre um alvo privado. Credenciais administrativas são entregues diretamente ao Vault.
    </p>
  </div>
</div>

{#if error}<p class="notice notice-error" role="alert">{error}</p>{/if}
{#if success}<p class="notice notice-success" role="status">{success}</p>{/if}

<div class="management-grid">
  <section class="panel table-panel">
    <div class="panel-heading">
      <div>
        <p class="eyebrow">CATÁLOGO</p>
        <h2>Alvos registrados</h2>
      </div>
      <span class="count-pill">{targets.length.toString().padStart(2, '0')}</span>
    </div>
    {#if loading}
      <div class="loading-panel">Lendo o catálogo…</div>
    {:else if targets.length === 0}
      <div class="empty-state compact-empty">
        <div class="empty-mark">▤</div>
        <strong>O catálogo está vazio</strong>
        <p>Cadastre o primeiro alvo usando o formulário ao lado.</p>
      </div>
    {:else}
      <div class="table-scroll">
        <table>
          <thead
            ><tr><th>ALVO</th><th>ENGINE</th><th>ENDPOINT</th><th>ESTADO</th><th></th></tr></thead
          >
          <tbody>
            {#each targets as target}
              <tr>
                <td
                  ><strong>{target.name}</strong><span class="cell-subtitle"
                    >{target.database_name} · {target.schema_name}</span
                  ></td
                >
                <td
                  ><span class="engine-chip"
                    >{target.engine === 'postgresql' ? 'PostgreSQL' : 'MySQL'}</span
                  ></td
                >
                <td class="mono-cell">{target.host}:{target.port}</td>
                <td
                  ><span
                    class="badge"
                    class:badge-live={target.enabled}
                    class:badge-muted={!target.enabled}>{target.enabled ? 'ATIVO' : 'PAUSADO'}</span
                  ></td
                >
                <td
                  ><button class="text-button" onclick={() => setEnabled(target)}
                    >{target.enabled ? 'Pausar' : 'Ativar'}</button
                  ></td
                >
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </section>

  <section class="panel form-panel">
    <div class="panel-heading">
      <div>
        <p class="eyebrow">NOVO ALVO</p>
        <h2>Registrar conexão</h2>
      </div>
      <span class="secure-stamp">TLS VERIFIED</span>
    </div>
    <form onsubmit={submit}>
      <label
        >Nome de exibição<input
          bind:value={name}
          required
          maxlength="80"
          placeholder="ex.: warehouse-prod"
        /></label
      >
      <div class="form-row">
        <label
          >Engine
          <select value={engine} onchange={changeEngine}>
            <option value="postgresql">PostgreSQL</option><option value="mysql">MySQL</option>
          </select>
        </label>
        <label>Porta<input bind:value={port} type="number" min="1" max="65535" required /></label>
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
        <label>Schema<input bind:value={schemaName} maxlength="63" placeholder="public" /></label>
      </div>
      <div class="form-divider"><span>ACESSO DE PROVISIONAMENTO</span><span>01</span></div>
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
        <input bind:value={rootPassword} type="password" required autocomplete="new-password" />
        <small>Usada apenas para configurar o Vault; não é armazenada no gateway.</small>
      </label>
      <details class="advanced-fields">
        <summary>CA TLS privado <span>opcional</span></summary>
        <label
          >Certificado CA PEM
          <textarea
            bind:value={tlsCaPem}
            rows="5"
            spellcheck="false"
            placeholder="-----BEGIN CERTIFICATE-----"
          ></textarea>
          <small
            >Use quando o banco for assinado por uma CA privada. A validação do hostname permanece
            ativa.</small
          >
        </label>
      </details>
      <button class="button button-primary button-wide" type="submit" disabled={saving}>
        {saving ? 'Configurando no Vault…' : 'Validar e registrar'}<span aria-hidden="true">→</span>
      </button>
    </form>
  </section>
</div>
