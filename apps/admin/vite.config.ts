import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vitest/config';
import { loadEnv } from 'vite';

export default defineConfig(({ mode }) => {
  const { VITE_ALLOWED_HOSTS = '' } = loadEnv(mode, '.', 'VITE_');
  const allowedHosts = VITE_ALLOWED_HOSTS.split(',')
    .map((host) => host.trim())
    .filter((host) => host.length > 0);

  return {
    plugins: [sveltekit()],
    server: {
      allowedHosts,
      proxy: {
        '/v1': 'http://127.0.0.1:8080',
        '/mcp': 'http://127.0.0.1:8080'
      }
    },
    test: {
      environment: 'node'
    }
  };
});
