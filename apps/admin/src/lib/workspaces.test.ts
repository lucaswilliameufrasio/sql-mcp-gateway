import { describe, expect, it } from 'vitest';
import {
  environmentLabel,
  showsEnvironmentTag,
  mcpEndpoint,
  mcpServerName,
  switchUrl,
  workspaceDirectory
} from './workspaces';

const advertised = {
  current_workspace: 'staging',
  workspaces: [
    {
      slug: 'dev',
      name: 'Desenvolvimento',
      environment: 'development',
      url: 'https://sql-dev.example.test/'
    },
    { slug: 'staging', url: 'https://sql-staging.example.test/' },
    { slug: 'prod', name: 'Produção', environment: 'production', url: 'https://sql.example.test/' }
  ]
};

describe('workspace directory', () => {
  it('separates this deployment from its siblings in configured order', () => {
    const directory = workspaceDirectory(advertised);
    expect(directory?.current).toEqual({
      slug: 'staging',
      name: 'staging',
      environment: undefined,
      origin: 'https://sql-staging.example.test'
    });
    expect(directory?.others.map((workspace) => workspace.slug)).toEqual(['dev', 'prod']);
    expect(directory?.others[1].environment).toBe('production');
  });

  it('is absent when the gateway advertises nothing or does not identify itself', () => {
    expect(workspaceDirectory({})).toBeUndefined();
    expect(workspaceDirectory({ ...advertised, current_workspace: 'qa' })).toBeUndefined();
    expect(workspaceDirectory({ ...advertised, current_workspace: undefined })).toBeUndefined();
  });

  it('drops entries that could not be safe links', () => {
    const directory = workspaceDirectory({
      current_workspace: 'prod',
      workspaces: [
        { slug: 'prod', url: 'https://sql.example.test/' },
        { slug: 'script', url: 'javascript:alert(1)' },
        { slug: 'Bad Slug', url: 'https://bad.example.test/' },
        { slug: 'broken', url: 'not a url' },
        { slug: 'odd', environment: 'qa', url: 'https://odd.example.test/' },
        null
      ] as never
    });
    expect(directory?.others).toEqual([
      { slug: 'odd', name: 'odd', environment: undefined, origin: 'https://odd.example.test' }
    ]);
  });

  it('keeps the current page when switching, except during the sign-in callback', () => {
    const [dev] = workspaceDirectory(advertised)!.others;
    expect(switchUrl(dev, '/databases/new')).toBe('https://sql-dev.example.test/databases/new');
    expect(switchUrl(dev, '/auth/callback')).toBe('https://sql-dev.example.test/');
    expect(switchUrl(dev, '//evil.example.test')).toBe(
      'https://sql-dev.example.test//evil.example.test'
    );
  });

  it('names MCP servers per workspace and keeps the plain name without workspaces', () => {
    const { current } = workspaceDirectory(advertised)!;
    expect(mcpServerName(current)).toBe('sql-mcp-gateway-staging');
    expect(mcpServerName(undefined)).toBe('sql-mcp-gateway');
    expect(mcpEndpoint(current)).toBe('https://sql-staging.example.test/mcp');
    expect(environmentLabel('production')).toBe('Produção');
    expect(environmentLabel(undefined)).toBe('Ambiente');
    const [dev, prod] = workspaceDirectory(advertised)!.others;
    expect(showsEnvironmentTag(dev)).toBe(false);
    expect(showsEnvironmentTag({ ...prod, name: 'prod' })).toBe(true);
    expect(showsEnvironmentTag(current)).toBe(false);
  });
});
