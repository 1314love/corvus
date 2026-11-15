import { defineConfig } from '@playwright/test';
import { resolve } from 'node:path';

const chromePath = process.env.CORVUS_CHROME_PATH;
const extensionPath = resolve(process.cwd(), 'build/extension');

export default defineConfig({
  testDir: './tests/e2e',
  timeout: 60000,
  expect: { timeout: 15000 },
  fullyParallel: false,
  workers: 1,
  reporter: [['list']],
  use: {
    headless: true,
    launchOptions: {
      executablePath: chromePath || undefined,
      args: ['--disable-extensions-except=' + extensionPath, '--load-extension=' + extensionPath],
    },
  },
});
