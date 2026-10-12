import { browser } from '$app/environment';
import { goto } from '$app/navigation';
import { UserManager, WebStorageStateStore, type User } from 'oidc-client-ts';
import type { AuthConfig, MeResponse, Principal } from './types';

export const session = $state({
  accessToken: undefined as string | undefined,
  currentPrincipal: undefined as Principal | undefined,
  isInitializing: true,
  sessionError: undefined as string | undefined
});

let manager: UserManager | undefined;
let initialization: Promise<void> | undefined;

export function initializeSession(): Promise<void> {
  if (!initialization) {
    initialization = initialize();
  }
  return initialization;
}

export async function signIn(): Promise<void> {
  await initializeSession();
  if (!manager) {
    throw new Error('A configuração de identidade não está disponível.');
  }
  await manager.signinRedirect();
}

export async function signOut(): Promise<void> {
  session.accessToken = undefined;
  session.currentPrincipal = undefined;
  session.sessionError = undefined;
  if (manager) {
    await manager.removeUser();
  }
}

export async function waitForSession(): Promise<void> {
  await initializeSession();
}

async function initialize(): Promise<void> {
  if (!browser) {
    session.isInitializing = false;
    return;
  }

  try {
    const response = await fetch('/v1/auth/config', { cache: 'no-store' });
    if (!response.ok) {
      throw new Error('Não foi possível carregar a configuração de identidade.');
    }
    const config = (await response.json()) as AuthConfig;
    manager = new UserManager({
      authority: config.issuer,
      client_id: config.client_id,
      redirect_uri: `${window.location.origin}/auth/callback`,
      response_type: 'code',
      scope: config.scopes.join(' '),
      extraQueryParams: { resource: config.resource },
      userStore: new WebStorageStateStore({ store: window.sessionStorage }),
      stateStore: new WebStorageStateStore({ store: window.sessionStorage }),
      automaticSilentRenew: false,
      loadUserInfo: false
    });

    if (window.location.pathname === '/auth/callback') {
      const user = await manager.signinRedirectCallback();
      await acceptUser(user);
      await goto('/', { replaceState: true });
    } else {
      const user = await manager.getUser();
      if (user && !user.expired) {
        await acceptUser(user);
      } else if (user) {
        await manager.removeUser();
      }
    }
  } catch {
    session.sessionError = 'O login não foi concluído. Tente novamente ou contate o administrador.';
  } finally {
    session.isInitializing = false;
  }
}

async function acceptUser(user: User): Promise<void> {
  if (!user.access_token || user.expired) {
    throw new Error('O provedor não retornou um access token válido.');
  }

  session.accessToken = user.access_token;
  const response = await fetch('/v1/me', {
    headers: { Authorization: `Bearer ${user.access_token}` },
    cache: 'no-store'
  });
  if (!response.ok) {
    session.accessToken = undefined;
    await manager?.removeUser();
    throw new Error('A identidade foi autenticada, mas o gateway recusou a sessão.');
  }
  const me = (await response.json()) as MeResponse;
  session.currentPrincipal = me.principal;
}

export async function apiFetch(path: string, init: RequestInit = {}): Promise<Response> {
  await initializeSession();
  const token = session.accessToken;
  if (!token) {
    throw new Error('Entre com sua identidade para continuar.');
  }

  const headers = new Headers(init.headers);
  headers.set('Authorization', `Bearer ${token}`);
  if (init.body && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json');
  }
  const response = await fetch(path, { ...init, headers, cache: 'no-store' });
  if (response.status === 401) {
    await signOut();
    throw new Error('Sua sessão expirou. Entre novamente.');
  }
  if (!response.ok) {
    const body = (await response.json().catch(() => undefined)) as
      { message?: string; error_code?: string } | undefined;
    const error = new Error(body?.message ?? 'Não foi possível concluir a operação.');
    Object.assign(error, { code: body?.error_code, status: response.status });
    throw error;
  }
  return response;
}

export function clearSessionError(): void {
  session.sessionError = undefined;
}
