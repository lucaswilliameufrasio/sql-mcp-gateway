import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [sveltekit()],
  server: {
    proxy: {
      '/v1': 'http://127.0.0.1:8080',
      '/mcp': 'http://127.0.0.1:8080'
    }
  },
  test: {
    environment: 'node'
  }
});
