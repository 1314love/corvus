import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import { resolve } from 'node:path';

export default defineConfig({
  root: resolve(process.cwd(), 'src/ui'),
  base: './',
  plugins: [vue()],
  resolve: { alias: { '@': resolve(process.cwd(), 'src') } },
  build: {
    outDir: resolve(process.cwd(), 'build/extension'),
    emptyOutDir: false,
    sourcemap: false,
    target: 'chrome116',
    rollupOptions: {
      input: {
        popup: resolve(process.cwd(), 'src/ui/popup.html'),
        options: resolve(process.cwd(), 'src/ui/options.html'),
        offscreen: resolve(process.cwd(), 'src/ui/offscreen.html'),
      },
      output: {
        entryFileNames: 'assets/[name]-[hash].js',
        chunkFileNames: 'assets/chunk-[hash].js',
        assetFileNames: 'assets/[name]-[hash][extname]',
      },
    },
  },
});
