import { describe, expect, it } from 'vitest';
import { claudeCodeCommand, endpointList, openCodeConfig, shellArgument } from './client-setup';

const single = [{ name: 'sql-mcp-gateway', endpoint: 'https://gateway.example.test/mcp' }];
const several = [
  { name: 'sql-mcp-gateway-dev', endpoint: 'https://sql-dev.example.test/mcp' },
  { name: 'sql-mcp-gateway-prod', endpoint: 'https://sql.example.test/mcp' }
];

describe('MCP onboarding configuration', () => {
  it('produces parseable OpenCode configuration with only the remote endpoint and no credentials', () => {
    expect(JSON.parse(openCodeConfig(single))).toEqual({
      $schema: 'https://opencode.ai/config.json',
      mcp: {
        servers: { 'sql-mcp-gateway': { type: 'remote', url: 'https://gateway.example.test/mcp' } }
      }
    });
    expect(openCodeConfig(single)).not.toMatch(/token|password|Authorization/i);
  });
  it('registers each workspace under its own server name', () => {
    expect(Object.keys(JSON.parse(openCodeConfig(several)).mcp.servers)).toEqual([
      'sql-mcp-gateway-dev',
      'sql-mcp-gateway-prod'
    ]);
    expect(claudeCodeCommand(several)).toBe(
      "claude mcp add --transport http 'sql-mcp-gateway-dev' 'https://sql-dev.example.test/mcp'\n" +
        "claude mcp add --transport http 'sql-mcp-gateway-prod' 'https://sql.example.test/mcp'"
    );
    expect(endpointList(single)).toBe('https://gateway.example.test/mcp');
    expect(endpointList(several)).toBe(
      'sql-mcp-gateway-dev  https://sql-dev.example.test/mcp\n' +
        'sql-mcp-gateway-prod  https://sql.example.test/mcp'
    );
  });
  it('quotes shell metacharacters and apostrophes instead of treating endpoints as commands', () => {
    expect(shellArgument('https://example.test/mcp?x=$(whoami)&y=1')).toBe(
      "'https://example.test/mcp?x=$(whoami)&y=1'"
    );
    expect(shellArgument("a'b")).toBe("'a'\\''b'");
  });
});
