import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

export default defineConfig({
  plugins: [vue()],
  server: {
    port: 8082,
    strictPort: true,
    proxy: Object.fromEntries(['/api', '/oidc/catalog/callback']
      .map(path => [path, 'http://127.0.0.1:3002'])),
  },
  preview: { port: 8082, strictPort: true },
});
