export type ClientChoice = { type: 'opencode' } | { type: 'claude-code' } | { type: 'other' };

export function openCodeConfig(endpoint: string): string {
  return JSON.stringify(
    {
      $schema: 'https://opencode.ai/config.json',
      mcp: { servers: { 'sql-mcp-gateway': { type: 'remote', url: endpoint } } }
    },
    undefined,
    2
  );
}

export function shellArgument(value: string): string {
  return `'${value.replaceAll("'", "'\\''")}'`;
}

export function claudeCodeCommand(endpoint: string): string {
  return `claude mcp add --transport http sql-mcp-gateway ${shellArgument(endpoint)}`;
}
