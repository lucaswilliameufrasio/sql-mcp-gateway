<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/state';
  import Icon from '$lib/components/Icon.svelte';
  import {
    clearSessionError,
    initializeSession,
    session,
    signIn,
    signOut
  } from '$lib/session.svelte';
  import '../app.css';

  let { children } = $props();
  let loginBusy = $state(false);
  let loginMessage = $state<string | undefined>();
  const navigation = [
    { href: '/', label: 'Visão geral', icon: 'home' },
    { href: '/connect', label: 'Conectar MCP', icon: 'plug' },
    { href: '/databases', label: 'Bancos de dados', icon: 'database' },
    { href: '/access', label: 'Pessoas e acessos', icon: 'users' },
    { href: '/audit', label: 'Auditoria', icon: 'audit' }
  ] as const;
  let currentPage = $derived(
    navigation.find((item) =>
      item.href === '/' ? page.url.pathname === '/' : page.url.pathname.startsWith(item.href)
    )?.label ?? 'Autenticação'
  );

  onMount(() => {
    void initializeSession();
  });

  async function handleSignIn(): Promise<void> {
    loginBusy = true;
    loginMessage = undefined;
    clearSessionError();
    try {
      await signIn();
    } catch {
      loginMessage =
        'Não foi possível iniciar o login. Verifique a configuração OIDC com o administrador.';
      loginBusy = false;
    }
  }
</script>

<svelte:head>
  <title>SQL MCP Gateway</title>
  <meta
    name="description"
    content="Conecte seu assistente a bancos SQL com acesso individual e auditável."
  />
</svelte:head>

{#if session.isInitializing}
  <main class="boot-screen" aria-live="polite">
    <div class="brand-mark"><Icon name="database" size={26} /></div>
    <p>Preparando sua sessão…</p>
  </main>
{:else if !session.currentPrincipal}
  <main class="login-screen">
    <section class="login-card">
      <div class="brand-lockup">
        <div class="brand-mark"><Icon name="database" size={25} /></div>
        <div>
          <strong>SQL MCP Gateway</strong>
          <p class="brand-subtitle">Seu ponto de acesso aos dados</p>
        </div>
      </div>
      <div class="login-copy">
        <h1>Seus dados.<br />Seu assistente.<br />O acesso certo.</h1>
        <p>
          Conecte seu cliente MCP aos bancos da equipe, com sua identidade e apenas as permissões de
          que você precisa.
        </p>
      </div>
      {#if session.sessionError || loginMessage}<p class="notice notice-error" role="alert">
          {loginMessage ?? session.sessionError}
        </p>{/if}
      <button class="button button-primary button-wide" onclick={handleSignIn} disabled={loginBusy}
        >{loginBusy ? 'Redirecionando…' : 'Entrar com SSO'}<Icon name="arrow" size={18} /></button
      >
      <p class="login-footnote">
        <Icon name="shield" size={16} /> Use a conta da sua organização. Sem uma nova senha.
      </p>
    </section>
    <aside class="login-aside" aria-label="Como funciona o acesso">
      <span class="login-aside-label">Do seu cliente ao banco, com controle.</span>
      <div class="connection-diagram">
        <div class="diagram-node">
          <Icon name="plug" size={26} />
          <div>
            <strong>Seu cliente MCP</strong><span>OpenCode, Claude Code ou outro cliente</span>
          </div>
        </div>
        <div class="diagram-line"><span>Login com sua identidade</span></div>
        <div class="diagram-node diagram-gateway">
          <Icon name="shield" size={28} />
          <div><strong>SQL MCP Gateway</strong><span>Permissões por pessoa e por banco</span></div>
        </div>
        <div class="diagram-line"><span>Credenciais temporárias</span></div>
        <div class="diagram-node">
          <Icon name="database" size={26} />
          <div><strong>PostgreSQL e MySQL</strong><span>Consultas e operações auditadas</span></div>
        </div>
      </div>
      <p>Para quem opera, controle e rastreabilidade.<br />Para quem usa, uma conexão simples.</p>
    </aside>
  </main>
{:else}
  <a class="skip-link" href="#main-content">Pular para o conteúdo</a>
  <div class="application-shell">
    <aside class="sidebar">
      <a class="brand-lockup sidebar-brand" href="/" aria-label="SQL MCP Gateway — início"
        ><div class="brand-mark"><Icon name="database" size={23} /></div>
        <div>
          <strong>SQL MCP</strong>
          <p class="brand-subtitle">Gateway</p>
        </div></a
      >
      <nav aria-label="Navegação principal">
        <p class="nav-group-label">Workspace</p>
        {#each navigation as item, index}
          {#if index < 2 || session.currentPrincipal.platform_role === 'admin'}
            {#if index === 2}<p class="nav-group-label">Administração</p>{/if}
            {@const active =
              item.href === '/'
                ? page.url.pathname === '/'
                : page.url.pathname.startsWith(item.href)}
            <a class:active href={item.href} aria-current={active ? 'page' : undefined}
              ><Icon name={item.icon} size={19} /><span>{item.label}</span></a
            >
          {/if}
        {/each}
      </nav>
      <div class="sidebar-bottom">
        <a class="sidebar-help" href="/connect"
          ><Icon name="plug" size={18} />
          <div><strong>Pronto para consultar?</strong><span>Configure seu cliente MCP</span></div>
          <Icon name="chevron" size={15} /></a
        >
        <div class="user-card">
          <div class="avatar">
            {(session.currentPrincipal.display_name ?? session.currentPrincipal.email ?? 'U')
              .slice(0, 1)
              .toUpperCase()}
          </div>
          <div class="user-details">
            <strong
              >{session.currentPrincipal.display_name ??
                session.currentPrincipal.email ??
                session.currentPrincipal.subject}</strong
            ><span
              >{session.currentPrincipal.platform_role === 'admin'
                ? 'Administrador'
                : 'Membro'}</span
            >
          </div>
          <button
            class="icon-button"
            aria-label="Sair da conta"
            title="Sair da conta"
            onclick={signOut}><Icon name="logout" size={18} /></button
          >
        </div>
      </div>
    </aside>
    <main class="main-panel" id="main-content" tabindex="-1">
      <header class="topbar">
        <div class="breadcrumbs">
          <span>Workspace</span><Icon name="chevron" size={14} /><span>{currentPage}</span
          >{#if page.url.pathname === '/databases/new'}<Icon name="chevron" size={14} /><span
              >Novo banco</span
            >{/if}
        </div>
        <span class="session-indicator"><Icon name="shield" size={15} /> Sessão autenticada</span>
      </header>
      <div class="page-content">{@render children()}</div>
      <footer class="workspace-footer">
        <span>SQL MCP Gateway</span><span>Acesso individual. Operações auditadas.</span>
      </footer>
    </main>
  </div>
{/if}
