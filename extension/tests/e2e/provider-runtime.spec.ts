import { chromium, expect, test } from '@playwright/test';
import { existsSync } from 'node:fs';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { encodeBase64Url } from '../../src/shared/provider/protocol';
import { startFixtureServer } from './support/fixture-server';
import { startProviderGatewayServer } from './support/provider-gateway-server';

const DEFAULT_CHROME = join(tmpdir(), 'corvus-chrome', 'chrome-win64', 'chrome.exe');
const chromePath = process.env.CORVUS_CHROME_PATH || DEFAULT_CHROME;
const PROVIDER_PORT = 29330;

test('built provider-only extension serves tabs, page, input and capture from about:blank', async () => {
  test.skip(!existsSync(chromePath), 'Chrome for Testing binary is not available');
  test.setTimeout(180000);

  const gatewayServer = await startProviderGatewayServer(PROVIDER_PORT);
  const fixture = await startFixtureServer(29332);
  const profile = await mkdtemp(join(tmpdir(), 'corvus-p2-e2e-'));
  let context: Awaited<ReturnType<typeof chromium.launchPersistentContext>> | null = null;

  try {
    context = await chromium.launchPersistentContext(profile, {
      headless: true,
      executablePath: chromePath,
      args: [
        '--disable-extensions-except=' + join(process.cwd(), 'build/extension'),
        '--load-extension=' + join(process.cwd(), 'build/extension'),
        '--no-first-run',
        '--no-default-browser-check',
        '--disable-gpu',
        '--disable-dev-shm-usage',
      ],
    });

    const initial = context.pages()[0] ?? (await context.newPage());
    expect(initial.url()).toBe('about:blank');

    let [worker] = context.serviceWorkers();
    if (!worker) worker = await context.waitForEvent('serviceworker', { timeout: 30000 });
    const extensionId = new URL(worker.url()).host;
    expect(extensionId).toMatch(/^[a-p]{32}$/);

    const options = await context.newPage();
    await options.goto('chrome-extension://' + extensionId + '/options.html');
    await expect(options.getByText('Local pairing')).toBeVisible({ timeout: 20000 });
    const inputs = options.locator('.form input');
    await inputs.nth(0).fill('ws://127.0.0.1:' + String(PROVIDER_PORT));
    await inputs.nth(1).fill(encodeBase64Url(gatewayServer.token));
    await options.getByRole('button', { name: 'Save and connect' }).click();
    await expect.poll(() => gatewayServer.gateway?.manifest != null, { timeout: 30000 }).toBe(true);
    await expect(options.getByText('connected', { exact: true })).toBeVisible({ timeout: 30000 });
    const gateway = gatewayServer.gateway;
    if (!gateway) throw new Error('the provider gateway did not accept a connection');
    await expect
      .poll(
        async () => {
          try {
            await gateway.invoke('system.info.info', {}, 5000);
            return true;
          } catch {
            return false;
          }
        },
        { timeout: 20000 },
      )
      .toBe(true);

    const info = (await gateway.invoke('system.info.info', {}, 10000)) as Record<string, any>;
    expect(info.browser_family).toBe('Chrome');
    expect(info.capabilities.map((group: any) => group.group)).toEqual(['tabs', 'page', 'input', 'capture']);
    expect(info.limits).toEqual({ max_invoke_bytes: 1048576, max_result_bytes: 8388608 });

    const listed = (await gateway.invoke('tabs.list', {}, 10000)) as { tabs: any[]; count: number };
    expect(listed.count).toBeGreaterThan(0);
    expect(listed.tabs.some((tab) => tab.url === 'about:blank')).toBe(true);

    const pageEvent = context.waitForEvent('page');
    const opened = (await gateway.invoke('tabs.open', { url: fixture.origin + '/p2', active: true }, 15000)) as Record<
      string,
      any
    >;
    const tabPage = await pageEvent;
    await tabPage.waitForLoadState('load');
    expect(opened).toMatchObject({ url: fixture.origin + '/p2', active: true });
    expect(typeof opened.tab_id).toBe('string');
    await expect.poll(() => tabPage.url()).toBe(fixture.origin + '/p2');

    const read = (await gateway.invoke('page.read', { tab_id: opened.tab_id, max_chars: 5000 }, 15000)) as Record<
      string,
      any
    >;
    expect(read.content).toContain('P2 fixture');
    expect(read.content).not.toContain('hunter2');
    expect(read.truncated).toBe(false);
    expect(read.tab_id).toBe(opened.tab_id);

    const snapshot = (await gateway.invoke(
      'page.snapshot',
      { tab_id: opened.tab_id, max_nodes: 500 },
      15000,
    )) as Record<string, any>;
    expect(snapshot.tab_id).toBe(opened.tab_id);
    const actionNode = snapshot.nodes.find((node: any) => node.name === 'Run action');
    const shadowNode = snapshot.nodes.find((node: any) => node.name === 'Shadow action');
    const fieldNode = snapshot.nodes.find((node: any) => node.name === 'Message');
    const secretNode = snapshot.nodes.find((node: any) => node.name === 'Secret');
    expect(actionNode?.ref).toMatch(/^@e[1-9][0-9]*$/);
    expect(shadowNode?.ref).toMatch(/^@e[1-9][0-9]*$/);
    expect(fieldNode?.ref).toMatch(/^@e[1-9][0-9]*$/);
    expect(secretNode?.value).toBeUndefined();

    const clicked = (await gateway.invoke(
      'input.click',
      { tab_id: opened.tab_id, ref: actionNode.ref },
      10000,
    )) as Record<string, any>;
    expect(clicked).toEqual({ tab_id: opened.tab_id, ref: actionNode.ref, clicked: true });
    await expect
      .poll(() => tabPage.evaluate(() => document.getElementById('action')?.dataset.clicked ?? null))
      .toBe('yes');

    const typed = (await gateway.invoke(
      'input.type',
      { tab_id: opened.tab_id, ref: fieldNode.ref, text: 'hello', clear: true, submit: false },
      10000,
    )) as Record<string, any>;
    expect(typed).toEqual({
      tab_id: opened.tab_id,
      ref: fieldNode.ref,
      typed_chars: 5,
      submitted: false,
    });
    await expect
      .poll(() => tabPage.evaluate(() => (document.getElementById('field') as HTMLInputElement).value))
      .toBe('hello');

    const captured = (await gateway.invoke(
      'capture.viewport',
      { tab_id: opened.tab_id, format: 'jpeg', quality: 60 },
      20000,
    )) as Record<string, any>;
    expect(captured).toMatchObject({
      tab_id: opened.tab_id,
      url: fixture.origin + '/p2',
      format: 'jpeg',
      mime_type: 'image/jpeg',
    });
    expect(captured.width).toBeGreaterThan(0);
    expect(captured.height).toBeGreaterThan(0);
    expect(String(captured.data).length).toBeGreaterThan(1000);

    const defaultPng = (await gateway.invoke('capture.viewport', { tab_id: opened.tab_id }, 20000)) as Record<
      string,
      any
    >;
    expect(defaultPng).toMatchObject({
      tab_id: opened.tab_id,
      url: fixture.origin + '/p2',
      format: 'png',
      mime_type: 'image/png',
    });
    expect(String(defaultPng.data)).toMatch(/^iVBOR/);

    const navigated = (await gateway.invoke(
      'page.navigate',
      { tab_id: opened.tab_id, url: fixture.origin + '/p2?step=2', wait_until: 'load', timeout_ms: 15000 },
      20000,
    )) as Record<string, any>;
    expect(navigated).toMatchObject({ tab_id: opened.tab_id, url: fixture.origin + '/p2?step=2', status: 'loaded' });
    await expect.poll(() => tabPage.url()).toBe(fixture.origin + '/p2?step=2');

    const html = (await gateway.invoke(
      'page.read',
      { tab_id: opened.tab_id, mode: 'html', max_chars: 5000 },
      15000,
    )) as Record<string, any>;
    expect(html.content).toContain('P2 fixture');
    expect(html.content).toContain('value="***"');

    const closed = (await gateway.invoke('tabs.close', { tab_id: opened.tab_id }, 10000)) as Record<string, any>;
    expect(closed).toEqual({ tab_id: opened.tab_id, closed: true });
    await expect.poll(() => tabPage.isClosed(), { timeout: 10000 }).toBe(true);
  } finally {
    if (context) await context.close();
    await fixture.close();
    await gatewayServer.close();
    await rm(profile, { recursive: true, force: true }).catch(() => undefined);
  }
});
