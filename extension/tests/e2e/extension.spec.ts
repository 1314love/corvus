import { chromium, expect, test } from '@playwright/test';
import { existsSync } from 'node:fs';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { prepareIsolatedExtension } from './support/prepare-extension';
import { startMockDaemon } from './support/mock-daemon';
import { startFixtureServer } from './support/fixture-server';
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
  const fixture = await startFixtureServer(29332);
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
    expect(helloArgs.version).toBe('1.6.0');
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

    // Locale smoke: switch options to Chinese without a reload, verify the
    // choice persists to chrome.storage and that a fresh popup picks it up.
    await options.getByRole('button', { name: '中文', exact: true }).click();
    await expect(options.getByText('守护进程连接')).toBeVisible();
    await expect(options.getByText('浏览器 CDP 端点')).toBeVisible();
    expect(await options.evaluate(() => document.documentElement.lang)).toBe('zh-CN');
    const storedChinese = await worker.evaluate(async () => {
      const stored = await chrome.storage.local.get('corvus.locale');
      return stored['corvus.locale'] ?? null;
    });
    expect(storedChinese).toBe('zh-CN');

    const zhPopup = await context.newPage();
    await zhPopup.goto('chrome-extension://' + extensionId + '/popup.html');
    await expect(zhPopup.getByText('已连接').first()).toBeVisible({ timeout: 20000 });
    await expect(zhPopup.getByText('守护进程')).toBeVisible();
    expect(await zhPopup.evaluate(() => document.documentElement.lang)).toBe('zh-CN');

    await options.bringToFront();
    await options.getByRole('button', { name: 'English', exact: true }).click();
    await expect(options.getByText('Daemon connection')).toBeVisible();
    expect(await options.evaluate(() => document.documentElement.lang)).toBe('en');
    const storedEnglish = await worker.evaluate(async () => {
      const stored = await chrome.storage.local.get('corvus.locale');
      return stored['corvus.locale'] ?? null;
    });
    expect(storedEnglish).toBe('en');

    const enPopup = await context.newPage();
    await enPopup.goto('chrome-extension://' + extensionId + '/popup.html');
    await expect(enPopup.getByText('connected').first()).toBeVisible({ timeout: 20000 });
    expect(await enPopup.evaluate(() => document.documentElement.lang)).toBe('en');

    // Snapshot smoke: the top page owns an open shadow root, a same-origin
    // srcdoc frame, and a cross-origin frame served from localhost.
    const page = await context.newPage();
    await page.goto(fixture.origin + '/frames.html', { waitUntil: 'load' });
    await page.bringToFront();
    await page.waitForFunction(() => {
      const frame = document.getElementById('same-frame') as HTMLIFrameElement | null;
      return Boolean(frame && frame.contentDocument && frame.contentDocument.getElementById('frame-button'));
    });
    await page.waitForFunction(() =>
      Boolean(document.getElementById('shadow-host')?.shadowRoot?.getElementById('shadow-button')),
    );

    let snapshotResponse: Record<string, unknown> | null = null;
    let lastSnapshotFailure: Record<string, unknown> | null = null;
    let pollError: unknown = null;
    try {
      await expect
        .poll(
          async () => {
            try {
              const candidate = await daemon.sendCommand(
                'page_snapshot',
                { format: 'json', include_bbox: true },
                20000,
              );
              lastSnapshotFailure = candidate.error ? candidate : null;
              if (candidate.error) return false;
              snapshotResponse = candidate;
              return true;
            } catch {
              return false;
            }
          },
          { timeout: 20000, intervals: [250, 500, 1000] },
        )
        .toBe(true);
    } catch (error) {
      pollError = error;
    }

    if (!snapshotResponse) {
      throw new Error(
        'page_snapshot did not produce a response: ' +
          JSON.stringify({
            lastSnapshotFailure,
            pollError: pollError instanceof Error ? pollError.message : String(pollError),
            daemon: daemon.snapshot(),
          }),
      );
    }
    const snapshot = snapshotResponse as {
      args?: {
        nodes?: Array<{ ref?: string; name?: string; frame?: string; bbox?: { x: number; y: number } }>;
        frames?: {
          total?: number;
          included?: number;
          skipped?: Array<Record<string, unknown>>;
        };
      };
    };
    const nodes = snapshot.args?.nodes || [];
    const topNode = nodes.find((node) => node.name === 'Top Action');
    const shadowNode = nodes.find((node) => node.name === 'Shadow Action');
    const frameNode = nodes.find((node) => node.name === 'Frame Action');
    expect(topNode?.ref).toMatch(/^@e\d+$/);
    expect(shadowNode?.ref).toMatch(/^@e\d+$/);
    expect(frameNode?.ref).toMatch(/^@e\d+$/);
    expect(shadowNode?.frame).toBeUndefined();
    expect(frameNode?.frame).toBe('f0');
    expect(snapshot.args?.frames).toEqual({
      total: 2,
      included: 1,
      skipped: [
        {
          frame: 'f1',
          url: fixture.crossOrigin + '/frame.html',
          reason: 'cross_origin',
        },
      ],
    });
    expect(frameNode?.bbox).toBeTruthy();
    expect(Math.abs((frameNode?.bbox?.x || 0) - 117)).toBeLessThanOrEqual(1);
    expect(Math.abs((frameNode?.bbox?.y || 0) - 137)).toBeLessThanOrEqual(1);

    const compactResponse = await daemon.sendCommand('page_snapshot', { format: 'compact' }, 20000);
    expect(compactResponse.error).toBeUndefined();
    const compactText = String((compactResponse.args as Record<string, unknown>)?.text || '');
    expect(compactText).toContain('Shadow Action');
    expect(compactText).toContain('frame="f0"');

    const shadowClick = await daemon.sendCommand('browser_click', { ref: shadowNode?.ref }, 20000);
    expect(shadowClick.error).toBeUndefined();
    await expect
      .poll(() =>
        page.evaluate(() => {
          const shadowRoot = document.getElementById('shadow-host')?.shadowRoot;
          const button = shadowRoot?.getElementById('shadow-button');
          return button?.dataset.clicked || null;
        }),
      )
      .toBe('yes');

    const frameClick = await daemon.sendCommand('browser_click', { ref: frameNode?.ref }, 20000);
    expect(frameClick.error).toBeUndefined();
    await expect
      .poll(() =>
        page.evaluate(() => {
          const frame = document.getElementById('same-frame') as HTMLIFrameElement | null;
          const button = frame?.contentDocument?.getElementById('frame-button');
          return button?.dataset.clicked || null;
        }),
      )
      .toBe('yes');
  } finally {
    if (context) await context.close();
    await prepared.cleanup();
    await daemon.close();
    await fixture.close();
    await rm(profile, { recursive: true, force: true });
  }
});
