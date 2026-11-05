import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

// Deliberately separate from vite.config.ts: production never loads fixture auth.
export default defineConfig({
  plugins: [
    {
      name: 'isolated-visual-fixtures',
      enforce: 'pre',
      apply: 'serve',
      configureServer(server) {
        server.middlewares.use('/__visual', async (_request, response, next) => {
          try {
            const html = await readFile(
              fileURLToPath(new URL('./tests/visual/viewport.html', import.meta.url)),
              'utf8'
            );
            response.setHeader('Content-Type', 'text/html; charset=utf-8');
            response.end(html);
          } catch (cause) {
            next(
              cause instanceof Error ? cause : new Error('Cannot load visual viewport harness.')
            );
          }
        });
      },
      async load(id) {
        if (id.endsWith('/src/lib/session.svelte.ts')) {
          return readFile(
            fileURLToPath(new URL('./tests/visual/session.fixture.txt', import.meta.url)),
            'utf8'
          );
        }
      }
    },
    sveltekit()
  ],
  server: { host: '127.0.0.1', strictPort: true }
});
