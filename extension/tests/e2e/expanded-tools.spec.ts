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

test('built extension executes the v1.7 tool expansion end to end', async () => {
  test.skip(!existsSync(chromePath), 'Chrome for Testing binary is not available');
  test.setTimeout(180000);

  const daemon = await startMockDaemon(29330);
  const fixture = await startFixtureServer(29332);
  const prepared = await prepareIsolatedExtension(join(process.cwd(), 'build/extension'), 29330, 29331);
  const profile = await mkdtemp(join(tmpdir(), 'corvus-e2e-expanded-'));
  let chrome: ChildProcess | null = null;
  let chromeLog = '';

  const call = async (tool: string, args: Record<string, unknown>, timeoutMs = 30000): Promise<any> => {
    const response = await daemon.sendCommand(tool, args, timeoutMs);
    if (response.error) throw new Error(tool + ' failed: ' + JSON.stringify(response.error));
    return response.args;
  };

  try {
    chrome = launchHeadlessChrome(prepared.dir, profile, fixture.origin + '/frames.html');
    chrome.stderr?.on('data', (chunk) => {
      chromeLog += String(chunk);
    });

    await expect.poll(() => daemon.hellos.length, { timeout: 30000 }).toBeGreaterThan(0);

    let snapshot: any = null;
    await expect
      .poll(
        async () => {
          try {
            const candidate = await daemon.sendCommand('page_snapshot', { format: 'json' }, 10000);
            if (candidate.error) return false;
            snapshot = candidate.args;
            return true;
          } catch {
            return false;
          }
        },
        { timeout: 30000, intervals: [250, 500, 1000] },
      )
      .toBe(true);

    const nodes = (snapshot?.nodes || []) as Array<Record<string, any>>;
    const refFor = (name: string): string => {
      const node = nodes.find((candidate) => candidate.name === name);
      expect(node?.ref, 'snapshot should expose ' + name).toMatch(/^@e\d+$/);
      return String(node?.ref);
    };
    const focusRef = refFor('Focus Me');
    const dragSourceRef = refFor('Drag source');
    const dragTargetRef = refFor('Drag target');

    const links = await call('page_links', { selector: '#links', max_results: 10 });
    expect(links).toMatchObject({
      url: fixture.origin + '/frames.html',
      count: 2,
      truncated: false,
    });
    expect(links.links).toEqual([
      { text: 'Docs', url: fixture.origin + '/docs', title: 'Documentation' },
      { text: 'Mail', url: 'mailto:help@example.com' },
    ]);
    const allLinks = await call('page_links', { selector: '#links', include_hidden: true });
    expect(allLinks.count).toBe(3);

    const forms = await call('page_forms', { selector: '#profile-form', include_values: true });
    expect(forms).toMatchObject({ count: 1, truncated: false });
    expect(forms.forms[0]).toMatchObject({
      action: fixture.origin + '/submit',
      method: 'post',
      id: 'profile-form',
    });
    const fields = forms.forms[0].fields as Array<Record<string, any>>;
    expect(fields.map((field) => field.name)).toEqual(['full_name', 'password', 'terms', 'plan', '']);
    expect(fields[0]).toMatchObject({ type: 'text', label: 'Full name', value: 'Ada' });
    expect(fields[1]).toMatchObject({ type: 'password', required: false });
    expect(fields[1]).not.toHaveProperty('value');
    expect(fields[2]).toMatchObject({ type: 'checkbox', checked: true });
    expect(fields[3].options).toEqual([
      { value: 'free', label: 'Free', selected: false },
      { value: 'pro', label: 'Pro', selected: true },
    ]);

    const passwordNode = nodes.find((candidate) => candidate.attributes?.type === 'password');
    expect(passwordNode).toBeTruthy();
    expect(passwordNode?.name || '').not.toContain('secret');
    expect(passwordNode?.attributes?.value).toBe('***');
    const compactSnapshot = await call('page_snapshot', { format: 'compact' });
    expect(String(compactSnapshot.text)).not.toContain('secret');
    expect(String(compactSnapshot.text)).toContain('value="***"');
    const secretFind = await call('page_find', { query: 'secret' });
    expect(secretFind).toMatchObject({ count: 0, matches: [] });

    const accessibility = await call('page_accessibility', { root: '#focus-button', max_nodes: 50 });
    expect(accessibility).toMatchObject({ url: fixture.origin + '/frames.html' });
    expect(accessibility.count).toBeGreaterThan(0);
    expect(
      (accessibility.nodes as Array<Record<string, any>>).some(
        (node) => node.role === 'button' && node.name === 'Focus Me',
      ),
    ).toBe(true);

    const pdf = await call('page_pdf', { paper_width: 8.5, paper_height: 11 }, 60000);
    expect(pdf).toMatchObject({
      format: 'pdf',
      mime_type: 'application/pdf',
      paper_width: 8.5,
      paper_height: 11,
      landscape: false,
      scale: 1,
      url: fixture.origin + '/frames.html',
    });
    expect(String(pdf.data).startsWith('JVBER')).toBe(true);
    expect(String(pdf.data).length).toBeGreaterThan(2000);

    const focused = await call('browser_focus', { ref: focusRef });
    expect(focused).toMatchObject({ focused: true, ref: focusRef, url: fixture.origin + '/frames.html' });

    const highlighted = await call('browser_highlight', { ref: focusRef, duration_ms: 50, color: '#ff0000' });
    expect(highlighted).toMatchObject({ highlighted: true, ref: focusRef, duration_ms: 50 });

    const dragged = await call('browser_drag', {
      from_ref: dragSourceRef,
      to_ref: dragTargetRef,
      steps: 2,
    });
    expect(dragged).toMatchObject({
      dragged: true,
      from: { ref: dragSourceRef },
      to: { ref: dragTargetRef },
      url: fixture.origin + '/frames.html',
    });
    expect(dragged.from.x).toBeCloseTo(680, 0);
    expect(dragged.from.y).toBeCloseTo(320, 0);
    expect(dragged.to.x).toBeCloseTo(880, 0);
    expect(dragged.to.y).toBeCloseTo(320, 0);

    const stored = await call('storage_set', {
      area: 'local',
      entries: { corvus_e2e: 'value-1' },
    });
    expect(stored).toMatchObject({ area: 'local', origin: fixture.origin, set: 1, keys: ['corvus_e2e'] });
    const read = await call('storage_get', { area: 'local', keys: ['corvus_e2e'] });
    expect(read).toMatchObject({
      area: 'local',
      origin: fixture.origin,
      count: 1,
      entries: [{ key: 'corvus_e2e', value: 'value-1' }],
    });
    const clearedStorage = await call('storage_clear', { area: 'local', keys: ['corvus_e2e'] });
    expect(clearedStorage).toMatchObject({ area: 'local', origin: fixture.origin, cleared: 1 });
    expect(JSON.stringify(clearedStorage)).not.toContain('value-1');

    const cookie = await call('cookie_set', {
      name: 'corvus_e2e_cookie',
      value: 'cookie-value',
      url: fixture.origin + '/frames.html',
      same_site: 'lax',
    });
    expect(cookie).toMatchObject({
      set: true,
      name: 'corvus_e2e_cookie',
      domain: '127.0.0.1',
      path: '/',
      same_site: 'lax',
      url: fixture.origin + '/frames.html',
    });
    expect(JSON.stringify(cookie)).not.toContain('cookie-value');
    const clearedCookie = await call('cookie_clear', {
      name: 'corvus_e2e_cookie',
      url: fixture.origin + '/frames.html',
    });
    expect(clearedCookie).toMatchObject({
      cleared: 1,
      name: 'corvus_e2e_cookie',
      url: fixture.origin + '/frames.html',
      names: ['corvus_e2e_cookie'],
    });

    const tabs = await call('tab_list', {});
    const sourceTab = (tabs.tabs as Array<Record<string, any>>).find(
      (tab) => tab.url === fixture.origin + '/frames.html',
    );
    expect(sourceTab?.tab_id).toMatch(/^\d+$/);
    const sourceTabId = String(sourceTab?.tab_id);

    const duplicated = await call('tab_duplicate', { tab_id: sourceTabId });
    expect(duplicated).toMatchObject({
      duplicated: true,
      source_tab_id: sourceTabId,
      tab: { url: fixture.origin + '/frames.html' },
    });
    const duplicateTabId = String(duplicated.tab.tab_id);

    const group = await call('tab_group', {
      tab_ids: [sourceTabId, duplicateTabId],
      title: 'E2E group',
      color: 'red',
    });
    expect(group).toMatchObject({
      tab_ids: [sourceTabId, duplicateTabId],
      title: 'E2E group',
      color: 'red',
    });
    expect(group.group_id).toMatch(/^\d+$/);
    expect(group.window_id).toMatch(/^\d+$/);

    const windows = await call('window_list', { include_tabs: true, max_windows: 20 });
    expect(windows.count).toBeGreaterThan(0);
    expect(Array.isArray(windows.windows)).toBe(true);
    expect(windows.windows.some((window: any) => window.window_id === group.window_id)).toBe(true);

    const created = await call('window_new', { url: 'about:blank', focused: false, width: 800, height: 600 });
    expect(created).toMatchObject({ created: true, incognito: false });
    expect(created.window_id).toMatch(/^\d+$/);
    expect(created.width).toBeGreaterThan(0);
    expect(created.height).toBeGreaterThan(0);
    expect(created.tab_id).toMatch(/^\d+$/);

    const targetUrl = fixture.origin + '/frames.html';
    const navigated = await call('browser_navigate', { tab_id: String(created.tab_id), url: targetUrl }, 30000);
    expect(navigated).toMatchObject({
      navigated: true,
      loaded: true,
      url: targetUrl,
      previous: 'about:blank',
    });
  } catch (error) {
    throw new Error(
      (error instanceof Error ? error.message : String(error)) +
        ' | chromeLog=' +
        JSON.stringify(chromeLog.slice(-1200)),
    );
  } finally {
    await stopChrome(chrome);
    await prepared.cleanup();
    await daemon.close();
    await fixture.close();
    await rm(profile, { recursive: true, force: true }).catch(() => undefined);
  }
});
