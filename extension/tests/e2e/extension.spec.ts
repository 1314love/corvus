import { chromium, expect, test } from '@playwright/test';
import { existsSync } from 'node:fs';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { prepareIsolatedExtension } from './support/prepare-extension';
import { startMockDaemon } from './support/mock-daemon';
import {
  collectExtensionDiagnostics,
  connectOffscreen,
  firstErrorLine,
  waitForOffscreenConnected,
} from './support/readiness';

const DEFAULT_CHROME = 'C:/Users/Administrator/AppData/Local/Temp/corvus-chrome/chrome-win64/chrome.exe';
const chromePath = process.env.CORVUS_CHROME_PATH || DEFAULT_CHROME;
const DAEMON_URL = 'ws://127.0.0.1:29330';

test('built extension connects to an isolated daemon and renders popup/options', async () => {
  test.skip(!existsSync(chromePath), 'Chrome for Testing binary is not available');
  test.setTimeout(120000);

  const daemon = await startMockDaemon(29330);
  const prepared = await prepareIsolatedExtension(join(process.cwd(), 'build/extension'), 29330, 29331);
  const profile = await mkdtemp(join(tmpdir(), 'corvus-e2e-profile-'));
  let context: Awaited<ReturnType<typeof chromium.launchPersistentContext>> | null = null;

  try {
    context = await chromium.launchPersistentContext(profile, {
      headless: true,
      executablePath: chromePath,
      args: [
        '--disable-extensions-except=' + prepared.dir,
        '--load-extension=' + prepared.dir,
        '--no-first-run',
        '--no-default-browser-check',
        '--disable-gpu',
        '--disable-dev-shm-usage',
      ],
    });

    let [worker] = context.serviceWorkers();
    if (!worker) worker = await context.waitForEvent('serviceworker', { timeout: 30000 });
    const extensionId = new URL(worker.url()).host;
    expect(extensionId).toMatch(/^[a-p]{32}$/);

    // Cold launches can start the service worker before the offscreen document has
    // finished creating its isolated socket. Force that state to be observable and
    // idempotently reconnect before waiting for the daemon-side hello.
    try {
      const ready = await waitForOffscreenConnected(worker, DAEMON_URL, 20000);
      expect(ready.status.status).toBe('connected');
      expect(ready.status.url).toBe(DAEMON_URL);
    } catch (error) {
      const diagnostics = await collectExtensionDiagnostics(worker, daemon);
      throw new Error(
        'extension offscreen was not ready: ' + JSON.stringify({ error: firstErrorLine(error), diagnostics }),
      );
    }

    let hello: Record<string, unknown>;
    try {
      hello = await daemon.waitForHello(12000);
    } catch (firstError) {
      const firstDiagnostics = await collectExtensionDiagnostics(worker, daemon);
      const retry = await connectOffscreen(worker, DAEMON_URL);
      try {
        hello = await daemon.waitForHello(12000);
      } catch (secondError) {
        const secondDiagnostics = await collectExtensionDiagnostics(worker, daemon);
        throw new Error(
          'daemon hello failed after one retry: ' +
            JSON.stringify({
              firstError: firstErrorLine(firstError),
              firstDiagnostics,
              retry,
              secondError: firstErrorLine(secondError),
              secondDiagnostics,
            }),
        );
      }
    }

    const helloArgs = (hello.args || {}) as Record<string, unknown>;
    expect(helloArgs.version).toBe('1.4.0');
    expect(Array.isArray(helloArgs.capabilities)).toBe(true);

    const popup = await context.newPage();
    await popup.goto('chrome-extension://' + extensionId + '/popup.html');
    await expect(popup.getByText('chromium:e2e')).toBeVisible({ timeout: 20000 });
    await expect(popup.getByText(/connected/i).first()).toBeVisible();
    await expect(popup.getByText('ws://127.0.0.1:29330')).toBeVisible();

    const options = await context.newPage();
    await options.goto('chrome-extension://' + extensionId + '/options.html');
    await expect(options.getByText('Daemon connection')).toBeVisible();
    await expect(options.getByText('Browser CDP endpoint')).toBeVisible();
    await expect(options.getByText('ws://127.0.0.1:29330').first()).toBeVisible();
  } finally {
    if (context) await context.close();
    await prepared.cleanup();
    await daemon.close();
    await rm(profile, { recursive: true, force: true });
  }
});
