export type ClientChoice = { type: 'opencode' } | { type: 'claude-code' } | { type: 'other' };

/** One MCP server entry for a client: the name it is saved under and its endpoint. */
export interface McpServer {
  name: string;
  endpoint: string;
}

export function openCodeConfig(servers: McpServer[]): string {
  return JSON.stringify(
    {
      $schema: 'https://opencode.ai/config.json',
      mcp: {
        servers: Object.fromEntries(
          servers.map((server) => [server.name, { type: 'remote', url: server.endpoint }])
        )
      }
    },
    undefined,
    2
  );
}

export function shellArgument(value: string): string {
  return `'${value.replaceAll("'", "'\\''")}'`;
}

export function claudeCodeCommand(servers: McpServer[]): string {
  return servers
    .map(
      (server) =>
        `claude mcp add --transport http ${shellArgument(server.name)} ${shellArgument(server.endpoint)}`
    )
    .join('\n');
}

export function endpointList(servers: McpServer[]): string {
  return servers.length === 1
    ? servers[0].endpoint
    : servers.map((server) => `${server.name}  ${server.endpoint}`).join('\n');
}
