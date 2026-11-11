import type { AuthConfig } from './types';

export type WorkspaceEnvironment = 'production' | 'staging' | 'development';

export interface Workspace {
  slug: string;
  name: string;
  environment?: WorkspaceEnvironment;
  /** Public origin of that gateway and its admin UI, without a trailing slash. */
  origin: string;
}

/** This deployment and the sibling deployments it advertises. */
export interface WorkspaceDirectory {
  current: Workspace;
  others: Workspace[];
}

const SLUG = /^[a-z0-9](?:[a-z0-9-]{0,30}[a-z0-9])?$/;
const ENVIRONMENTS: readonly string[] = ['production', 'staging', 'development'];
const ENVIRONMENT_LABELS: Record<WorkspaceEnvironment, string> = {
  production: 'Produção',
  staging: 'Homologação',
  development: 'Desenvolvimento'
};

function parseWorkspace(value: unknown): Workspace | undefined {
  if (typeof value !== 'object' || value === null) {
    return undefined;
  }
  const { slug, name, environment, url } = value as Record<string, unknown>;
  if (typeof slug !== 'string' || !SLUG.test(slug) || typeof url !== 'string') {
    return undefined;
  }
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return undefined;
  }
  // Switcher links must never become javascript: or data: navigations.
  if (parsed.protocol !== 'https:' && parsed.protocol !== 'http:') {
    return undefined;
  }
  return {
    slug,
    name: typeof name === 'string' && name.trim() ? name.trim() : slug,
    environment:
      typeof environment === 'string' && ENVIRONMENTS.includes(environment)
        ? (environment as WorkspaceEnvironment)
        : undefined,
    origin: parsed.origin
  };
}

/** Returns nothing when the gateway does not advertise workspaces. */
export function workspaceDirectory(
  config: Pick<AuthConfig, 'current_workspace' | 'workspaces'>
): WorkspaceDirectory | undefined {
  if (typeof config.current_workspace !== 'string' || !Array.isArray(config.workspaces)) {
    return undefined;
  }
  const workspaces = config.workspaces
    .map(parseWorkspace)
    .filter((workspace): workspace is Workspace => workspace !== undefined);
  const current = workspaces.find((workspace) => workspace.slug === config.current_workspace);
  if (!current) {
    return undefined;
  }
  return { current, others: workspaces.filter((workspace) => workspace !== current) };
}

export function environmentLabel(environment: WorkspaceEnvironment | undefined): string {
  return environment ? ENVIRONMENT_LABELS[environment] : 'Ambiente';
}

/** A name that already states the environment needs no tag repeating it. */
export function showsEnvironmentTag(workspace: Workspace): boolean {
  return (
    workspace.environment !== undefined &&
    workspace.name.toLocaleLowerCase('pt-BR') !==
      ENVIRONMENT_LABELS[workspace.environment].toLocaleLowerCase('pt-BR')
  );
}

/** Opens the same page in another workspace; the sign-in callback maps to the start page. */
export function switchUrl(workspace: Workspace, pathname: string): string {
  const path = pathname.startsWith('/') && !pathname.startsWith('/auth/') ? pathname : '/';
  return `${workspace.origin}${path}`;
}

export function mcpEndpoint(workspace: Workspace): string {
  return `${workspace.origin}/mcp`;
}

/** MCP clients key servers by name, so each workspace needs its own. */
export function mcpServerName(workspace: Workspace | undefined): string {
  return workspace ? `sql-mcp-gateway-${workspace.slug}` : 'sql-mcp-gateway';
}
