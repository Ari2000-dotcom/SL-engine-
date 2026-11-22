import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

const target = process.env.SLG_API ?? 'http://127.0.0.1:8787';

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      '/api': target,
      '/ws': { target, ws: true },
    },
  },
  css: { preprocessorOptions: { scss: { api: 'modern-compiler', quietDeps: true } } },
  build: { chunkSizeWarningLimit: 1500 },
});
