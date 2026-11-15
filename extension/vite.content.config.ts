import { defineConfig } from 'vite';
import { resolve } from 'node:path';

export default defineConfig({
  resolve: { alias: { '@': resolve(process.cwd(), 'src') } },
  build: {
    outDir: 'build/extension',
    emptyOutDir: false,
    sourcemap: false,
    target: 'chrome116',
    lib: {
      entry: resolve(process.cwd(), 'src/content/index.ts'),
      formats: ['iife'],
      name: 'CorvusContent',
      fileName: () => 'content.js',
    },
    rollupOptions: { output: { inlineDynamicImports: true, extend: false } },
  },
});
