import { describe, expect, it } from 'vitest';
import { claudeCodeCommand, openCodeConfig, shellArgument } from './client-setup';

describe('MCP onboarding configuration', () => {
  it('produces parseable OpenCode configuration with only the remote endpoint and no credentials', () => {
    const endpoint = 'https://gateway.example.test/mcp';
    expect(JSON.parse(openCodeConfig(endpoint))).toEqual({
      $schema: 'https://opencode.ai/config.json',
      mcp: { servers: { 'sql-mcp-gateway': { type: 'remote', url: endpoint } } }
    });
    expect(openCodeConfig(endpoint)).not.toMatch(/token|password|Authorization/i);
  });
  it('builds the HTTP client command with a quoted endpoint', () => {
    expect(claudeCodeCommand('https://gateway.example.test/mcp')).toBe(
      "claude mcp add --transport http sql-mcp-gateway 'https://gateway.example.test/mcp'"
    );
  });
  it('quotes shell metacharacters and apostrophes instead of treating endpoints as commands', () => {
    expect(shellArgument('https://example.test/mcp?x=$(whoami)&y=1')).toBe(
      "'https://example.test/mcp?x=$(whoami)&y=1'"
    );
    expect(shellArgument("a'b")).toBe("'a'\\''b'");
  });
});
