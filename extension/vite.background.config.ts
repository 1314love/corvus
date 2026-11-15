import { defineConfig } from 'vite';
import { resolve } from 'node:path';

export default defineConfig({
  resolve: { alias: { '@': resolve(process.cwd(), 'src') } },
  build: {
    outDir: 'build/extension',
    emptyOutDir: true,
    sourcemap: false,
    target: 'chrome116',
    lib: {
      entry: resolve(process.cwd(), 'src/background/index.ts'),
      formats: ['es'],
      fileName: () => 'background.js',
    },
    rollupOptions: { output: { inlineDynamicImports: true } },
  },
});
