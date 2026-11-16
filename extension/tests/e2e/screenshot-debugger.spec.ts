import { expect, test } from '@playwright/test';
import { spawn, execFileSync, type ChildProcess } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { prepareIsolatedExtension } from './support/prepare-extension';
import { startMockDaemon } from './support/mock-daemon';
import { startFixtureServer } from './support/fixture-server';

const DEFAULT_CHROME = 'C:/Users/Administrator/AppData/Local/Temp/corvus-chrome/chrome-win64/chrome.exe';
const chromePath = process.env.CORVUS_CHROME_PATH || DEFAULT_CHROME;

// Playwright keeps a CDP session attached to every page it controls, so
// chrome.debugger rejects an extension attach with debugger_attached_elsewhere.
// This smoke launches the same Chrome binary without an external debugger so the
// extension can exercise its real Page.captureScreenshot path.
function launchHeadlessChrome(extensionDir: string, profile: string, url: string): ChildProcess {
  const child = spawn(
    chromePath,
    [
      '--headless=new',
      '--no-sandbox',
      '--disable-setuid-sandbox',
      '--disable-gpu',
      '--disable-dev-shm-usage',
      '--no-first-run',
      '--no-default-browser-check',
      '--user-data-dir=' + profile,
      '--disable-extensions-except=' + extensionDir,
      '--load-extension=' + extensionDir,
      '--window-size=1280,720',
      url,
    ],
    { stdio: ['ignore', 'ignore', 'pipe'], windowsHide: true },
  );
  child.on('error', () => undefined);
  return child;
}

async function stopChrome(child: ChildProcess | null): Promise<void> {
  if (!child) return;
  if (process.platform === 'win32' && child.pid) {
    try {
      execFileSync('taskkill', ['/pid', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
    } catch {
      // The browser may already have exited.
    }
  } else {
    child.kill('SIGKILL');
  }
  if (child.exitCode === null) {
    await new Promise<void>((resolve) => {
      const timer = setTimeout(resolve, 5000);
      child.once('exit', () => {
        clearTimeout(timer);
        resolve();
      });
    });
  }
}

test('built extension captures screenshots through the real debugger channel', async () => {
  test.skip(!existsSync(chromePath), 'Chrome for Testing binary is not available');
  test.setTimeout(120000);

  const daemon = await startMockDaemon(29330);
  const fixture = await startFixtureServer(29332);
  const prepared = await prepareIsolatedExtension(join(process.cwd(), 'build/extension'), 29330, 29331);
  const profile = await mkdtemp(join(tmpdir(), 'corvus-e2e-screenshot-'));
  let chrome: ChildProcess | null = null;
  let chromeLog = '';

  try {
    chrome = launchHeadlessChrome(prepared.dir, profile, fixture.origin + '/frames.html');
    chrome.stderr?.on('data', (chunk) => {
      chromeLog += String(chunk);
    });

    await expect.poll(() => daemon.hellos.length, { timeout: 30000 }).toBeGreaterThan(0);

    let snapshot: any = null;
    let lastFailure: unknown = null;
    try {
      await expect
        .poll(
          async () => {
            try {
              const candidate = await daemon.sendCommand(
                'page_snapshot',
                { format: 'json', include_bbox: true },
                10000,
              );
              if (candidate.error) {
                lastFailure = candidate;
                return false;
              }
              snapshot = candidate.args as Record<string, any>;
              return true;
            } catch (error) {
              lastFailure = { message: error instanceof Error ? error.message : String(error) };
              return false;
            }
          },
          { timeout: 30000, intervals: [250, 500, 1000] },
        )
        .toBe(true);
    } catch (error) {
      let tabs: unknown = null;
      try {
        tabs = await daemon.sendCommand('tab_list', {}, 10000);
      } catch (tabError) {
        tabs = { message: tabError instanceof Error ? tabError.message : String(tabError) };
      }
      throw new Error(
        'manual page_snapshot did not become ready: ' +
          JSON.stringify({
            lastFailure,
            tabs,
            hellos: daemon.hellos,
            daemon: daemon.snapshot(),
            chromeLog: chromeLog.slice(-1000),
            pollError: error instanceof Error ? error.message : String(error),
          }),
      );
    }

    const nodes = (snapshot?.nodes || []) as Array<Record<string, any>>;
    const shadowNode = nodes.find((node) => node.name === 'Shadow Action');
    const frameNode = nodes.find((node) => node.name === 'Frame Action');
    expect(shadowNode?.ref).toMatch(/^@e\d+$/);
    expect(frameNode?.ref).toMatch(/^@e\d+$/);

    const viewportShot = await daemon.sendCommand('browser_screenshot', { target: 'viewport', format: 'webp' }, 30000);
    expect(viewportShot.error).toBeUndefined();
    const viewport = viewportShot.args as Record<string, any>;
    expect(viewport).toMatchObject({
      format: 'webp',
      mime_type: 'image/webp',
      target: 'viewport',
      url: fixture.origin + '/frames.html',
    });
    expect(typeof viewport.tab_id).toBe('string');
    expect(typeof viewport.data).toBe('string');
    expect(viewport.data.length).toBeGreaterThan(2000);
    expect(viewport).not.toHaveProperty('bbox');
    expect(viewport).not.toHaveProperty('inline');

    const inlineShot = await daemon.sendCommand(
      'browser_screenshot',
      { target: 'viewport', format: 'webp', inline: true },
      30000,
    );
    expect(inlineShot.error).toBeUndefined();
    const inlineImage = inlineShot.args as Record<string, any>;
    expect(inlineImage).toMatchObject({ format: 'webp', mime_type: 'image/webp', target: 'viewport' });
    expect(typeof inlineImage.data).toBe('string');
    expect(inlineImage.data.length).toBeGreaterThan(2000);
    expect(inlineImage).not.toHaveProperty('inline');

    const fullPageShot = await daemon.sendCommand('browser_screenshot', { target: 'full_page', format: 'png' }, 30000);
    expect(fullPageShot.error).toBeUndefined();
    const fullPage = fullPageShot.args as Record<string, any>;
    expect(fullPage).toMatchObject({ format: 'png', mime_type: 'image/png', target: 'full_page' });
    expect(fullPage.data.length).toBeGreaterThan(2000);
    expect(fullPage).not.toHaveProperty('bbox');

    const shadowShot = await daemon.sendCommand(
      'browser_screenshot',
      { target: 'element', ref: shadowNode?.ref, format: 'webp' },
      30000,
    );
    expect(shadowShot.error).toBeUndefined();
    const shadowImage = shadowShot.args as Record<string, any>;
    expect(shadowImage).toMatchObject({
      format: 'webp',
      mime_type: 'image/webp',
      target: 'element',
      bbox: { x: 30, y: 70, width: 140, height: 32 },
    });
    expect(shadowImage.data.length).toBeGreaterThan(200);

    const frameShot = await daemon.sendCommand(
      'browser_screenshot',
      { target: 'element', ref: frameNode?.ref, format: 'png' },
      30000,
    );
    expect(frameShot.error).toBeUndefined();
    const frameImage = frameShot.args as Record<string, any>;
    expect(frameImage).toMatchObject({
      format: 'png',
      mime_type: 'image/png',
      target: 'element',
      bbox: { x: 117, y: 137, width: 140, height: 32 },
    });
    expect(frameImage.data.length).toBeGreaterThan(200);

    const cdpRefShot = await daemon.sendCommand('browser_screenshot', { target: 'element', ref: '@c1' }, 10000);
    expect(cdpRefShot.error).toMatchObject({ code: 'BAD_ARGS' });

    const missingRefShot = await daemon.sendCommand(
      'browser_screenshot',
      { target: 'element', ref: '@e999999' },
      10000,
    );
    expect(missingRefShot.error).toMatchObject({ code: 'NOT_FOUND' });
  } finally {
    await stopChrome(chrome);
    await prepared.cleanup();
    await daemon.close();
    await fixture.close();
    await rm(profile, { recursive: true, force: true }).catch(() => undefined);
  }
});
