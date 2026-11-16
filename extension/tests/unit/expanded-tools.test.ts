import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  EXPANDED_TOOL_NAMES,
  createExpandedHandlers,
  normalizeAxTree,
  parseBrowserDragArgs,
  parseBrowserFocusArgs,
  parseBrowserHighlightArgs,
  parseCookieClearArgs,
  parseCookieSetArgs,
  parsePageAccessibilityArgs,
  parsePageFormsArgs,
  parsePageLinksArgs,
  parsePagePdfArgs,
  parseStorageClearArgs,
  parseStorageGetArgs,
  parseStorageSetArgs,
  parseTabDuplicateArgs,
  parseTabGroupArgs,
  parseWindowListArgs,
  parseWindowNewArgs,
} from '../../src/background/expanded-tools';
import { ToolError } from '../../src/shared/errors';

function expectBadArgs(run: () => any): void {
  try {
    run();
    throw new Error('expected BAD_ARGS');
  } catch (error: any) {
    expect(error).toBeInstanceOf(ToolError);
    expect(error.code).toBe('BAD_ARGS');
  }
}

async function expectToolError(promise: Promise<any>, code: string): Promise<void> {
  try {
    await promise;
    throw new Error('expected ' + code);
  } catch (error: any) {
    expect(error).toBeInstanceOf(ToolError);
    expect(error.code).toBe(code);
  }
}

function makeDeps(overrides: Record<string, any> = {}): any {
  const base: any = {
    resolveTab: vi.fn(async () => ({ id: 7, url: 'https://example.com/page', windowId: 2, active: true })),
    runInTab: vi.fn(async () => ({ delegated: true })),
    requestContentResult: vi.fn(async () => ({})),
    sendCdpCommand: vi.fn(async () => ({})),
    tabIdString: (id: any) => String(id),
  };
  return Object.assign(base, overrides);
}

afterEach(() => {
  delete (globalThis as any).chrome;
});

describe('expanded tool parsers', () => {
  it('exports exactly the 16 new handlers and applies contract defaults', () => {
    expect(EXPANDED_TOOL_NAMES).toEqual([
      'page_pdf',
      'page_links',
      'page_forms',
      'page_accessibility',
      'browser_drag',
      'browser_focus',
      'browser_highlight',
      'tab_group',
      'tab_duplicate',
      'window_list',
      'window_new',
      'cookie_set',
      'cookie_clear',
      'storage_get',
      'storage_set',
      'storage_clear',
    ]);
    const handlers = createExpandedHandlers(makeDeps());
    expect(Object.keys(handlers).sort()).toEqual([...EXPANDED_TOOL_NAMES].sort());
    for (const name of EXPANDED_TOOL_NAMES) expect(typeof handlers[name]).toBe('function');

    expect(parsePagePdfArgs({})).toEqual({
      paper_width: 8.5,
      paper_height: 11,
      landscape: false,
      print_background: true,
      scale: 1,
    });
    expect(parsePageLinksArgs({})).toEqual({ max_results: 200, include_hidden: false });
    expect(parsePageFormsArgs({})).toEqual({ include_values: false, max_forms: 20 });
    expect(parsePageAccessibilityArgs({})).toEqual({ max_nodes: 500, max_depth: 12 });
    expect(parseBrowserDragArgs({ from_ref: '@e1', to_ref: '@e2' })).toMatchObject({ steps: 10 });
    expect(parseBrowserFocusArgs({ ref: '@e3' })).toMatchObject({ ref: '@e3' });
    expect(parseBrowserHighlightArgs({ selector: '#x' })).toMatchObject({
      duration_ms: 1200,
      color: '#ffd400',
      selector: '#x',
    });
    expect(parseTabGroupArgs({ tab_ids: ['1'] })).toMatchObject({ color: 'blue', tab_ids: ['1'] });
    expect(parseTabDuplicateArgs({ tab_id: '3' })).toMatchObject({ tab_id: '3' });
    expect(parseWindowListArgs({})).toMatchObject({ include_tabs: true, max_windows: 20 });
    expect(parseWindowNewArgs({})).toMatchObject({ incognito: false, focused: true });
    expect(parseCookieSetArgs({ name: 'sid', value: '' })).toMatchObject({
      path: '/',
      secure: false,
      http_only: false,
      same_site: 'lax',
    });
    expect(parseCookieClearArgs({ name: 'sid' })).toMatchObject({ name: 'sid' });
    expect(parseStorageGetArgs({})).toMatchObject({ area: 'local' });
    expect(parseStorageSetArgs({ entries: { a: 'b' } })).toMatchObject({ area: 'local', entries: { a: 'b' } });
    expect(parseStorageClearArgs({ area: 'session' })).toMatchObject({ area: 'session' });
  });

  it('rejects unknown keys and every contract bound violation', () => {
    expectBadArgs(() => parsePagePdfArgs({ paper_width: 0 }));
    expectBadArgs(() => parsePagePdfArgs({ extra: true }));
    expectBadArgs(() => parsePageLinksArgs({ max_results: 1001 }));
    expectBadArgs(() => parsePageLinksArgs({ selector: '' }));
    expectBadArgs(() => parsePageFormsArgs({ max_forms: 0 }));
    expectBadArgs(() => parsePageAccessibilityArgs({ max_depth: 51 }));
    expectBadArgs(() => parseBrowserDragArgs({ from_ref: '@e1', to_ref: '@e1' }));
    expectBadArgs(() => parseBrowserDragArgs({ from_ref: '@e1' }));
    expectBadArgs(() => parseBrowserHighlightArgs({}));
    expectBadArgs(() => parseBrowserHighlightArgs({ ref: '@e1', selector: '#x' }));
    expectBadArgs(() => parseBrowserHighlightArgs({ selector: '#x', color: 'red' }));
    expectBadArgs(() => parseTabGroupArgs({ tab_ids: [] }));
    expectBadArgs(() => parseTabGroupArgs({ tab_ids: ['1'], color: 'white' }));
    expectBadArgs(() => parseWindowNewArgs({ url: 'file:///tmp/x' }));
    expectBadArgs(() => parseWindowNewArgs({ width: 100 }));
    expectBadArgs(() => parseCookieSetArgs({ name: 'x' }));
    expectBadArgs(() => parseCookieSetArgs({ name: 'x', value: 'y', same_site: 'always' }));
    expectBadArgs(() => parseStorageSetArgs({ entries: {} }));
    expectBadArgs(() => parseStorageSetArgs({ entries: { a: 1 } }));
    expectBadArgs(() => parseStorageSetArgs({ entries: { a: 'x'.repeat(4097) } }));
    expectBadArgs(() => parseStorageGetArgs({ keys: new Array(101).fill('k') }));
  });
});

describe('page and browser handlers', () => {
  it('prints PDF through the debugger channel without daemon file fields', async () => {
    const deps = makeDeps({ sendCdpCommand: vi.fn(async () => ({ data: 'JVBERi0=' })) });
    const handlers = createExpandedHandlers(deps);
    const result = await handlers.page_pdf({ paper_width: 10, landscape: true });
    expect(deps.sendCdpCommand).toHaveBeenCalledWith(7, 'Page.printToPDF', {
      printBackground: true,
      landscape: true,
      paperWidth: 10,
      paperHeight: 11,
      scale: 1,
      marginTop: 0.4,
      marginBottom: 0.4,
      marginLeft: 0.4,
      marginRight: 0.4,
      preferCSSPageSize: false,
      transferMode: 'ReturnAsBase64',
    });
    expect(result).toEqual({
      format: 'pdf',
      mime_type: 'application/pdf',
      tab_id: '7',
      url: 'https://example.com/page',
      data: 'JVBERi0=',
      paper_width: 10,
      paper_height: 11,
      landscape: true,
      scale: 1,
    });
    await expectToolError(
      createExpandedHandlers(makeDeps({ sendCdpCommand: vi.fn(async () => ({})) })).page_pdf({}),
      'INTERNAL',
    );
    await expectToolError(
      createExpandedHandlers(
        makeDeps({
          sendCdpCommand: vi.fn(async () => {
            throw new Error('not attached');
          }),
        }),
      ).page_pdf({}),
      'CAPABILITY_UNAVAILABLE',
    );
  });

  it('delegates extraction and storage tools with parsed arguments', async () => {
    const deps = makeDeps();
    const handlers = createExpandedHandlers(deps);
    const frame = { id: 'f1' };
    expect(await handlers.page_links({ selector: '#links', max_results: 5 }, frame)).toEqual({ delegated: true });
    expect(deps.runInTab).toHaveBeenLastCalledWith(
      'page_links',
      { selector: '#links', max_results: 5, include_hidden: false },
      frame,
    );
    await handlers.page_forms({ include_values: true });
    expect(deps.runInTab).toHaveBeenLastCalledWith('page_forms', { include_values: true, max_forms: 20 }, undefined);
    await handlers.storage_get({ area: 'session', keys: ['a'] });
    expect(deps.runInTab).toHaveBeenLastCalledWith('storage_get', { area: 'session', keys: ['a'] }, undefined);
    await handlers.storage_set({ entries: { a: 'b' } });
    expect(deps.runInTab).toHaveBeenLastCalledWith('storage_set', { area: 'local', entries: { a: 'b' } }, undefined);
    await handlers.storage_clear({ keys: ['a'] });
    expect(deps.runInTab).toHaveBeenLastCalledWith('storage_clear', { area: 'local', keys: ['a'] }, undefined);
    await handlers.browser_focus({ ref: '@e1' }, frame);
    expect(deps.runInTab).toHaveBeenLastCalledWith('browser_focus', { ref: '@e1' }, frame, { withTabId: true });
    await handlers.browser_highlight({ selector: '#x', color: '#00ff00' }, frame);
    expect(deps.runInTab).toHaveBeenLastCalledWith(
      'browser_highlight',
      { selector: '#x', duration_ms: 1200, color: '#00ff00' },
      frame,
      { withTabId: true },
    );
  });

  it('drags between ref centres with held-left interpolation', async () => {
    const calls: any[] = [];
    const deps = makeDeps({
      requestContentResult: vi.fn(async (_tab: any, message: any) =>
        message.ref === '@e1'
          ? { ref: '@e1', x: 10, y: 20, pageX: 10, pageY: 120 }
          : { ref: '@e2', x: 50, y: 80, pageX: 50, pageY: 180 },
      ),
      sendCdpCommand: vi.fn(async (_tab: any, method: string, params: any) => {
        calls.push([method, params]);
        return {};
      }),
    });
    const result = await createExpandedHandlers(deps).browser_drag({ from_ref: '@e1', to_ref: '@e2', steps: 2 });
    expect(result).toEqual({
      dragged: true,
      from: { ref: '@e1', x: 10, y: 120 },
      to: { ref: '@e2', x: 50, y: 180 },
      tab_id: '7',
      url: 'https://example.com/page',
    });
    expect(calls.map((entry) => entry[1].type)).toEqual([
      'mouseMoved',
      'mousePressed',
      'mouseMoved',
      'mouseMoved',
      'mouseReleased',
    ]);
    expect(calls[2][1]).toMatchObject({ x: 30, y: 50, buttons: 1 });
    expect(calls[3][1]).toMatchObject({ x: 50, y: 80, buttons: 1 });
    expect(calls[4][1]).toMatchObject({ x: 50, y: 80, buttons: 0 });
  });

  it('normalizes an accessibility tree and scopes it by backend node', async () => {
    const raw = [
      { nodeId: '1', role: { value: 'RootWebArea' }, name: { value: 'Page' }, childIds: ['2', '3'] },
      { nodeId: '2', parentId: '1', role: { value: 'none' }, childIds: ['4'] },
      { nodeId: '3', parentId: '1', role: { value: 'button' }, name: { value: 'Skip' }, ignored: true },
      {
        nodeId: '4',
        parentId: '2',
        role: { value: 'button' },
        name: { value: 'Target' },
        backendDOMNodeId: 44,
        properties: [{ name: 'disabled', value: { value: true } }],
        childIds: ['5'],
      },
      {
        nodeId: '5',
        parentId: '4',
        role: { value: 'StaticText' },
        name: { value: 'Label' },
        properties: [{ name: 'level', value: { value: 2 } }],
      },
    ];
    const normalized = normalizeAxTree(raw, null, 20, 20);
    expect(normalized.nodes.map((node) => [node.role, node.name, node.depth])).toEqual([
      ['rootwebarea', 'Page', 0],
      ['button', 'Target', 1],
      ['statictext', 'Label', 2],
    ]);
    expect(normalized.truncated).toBe(false);
    const scoped = normalizeAxTree(raw, 44, 20, 20);
    expect(scoped.nodes.map((node) => node.name)).toEqual(['Target', 'Label']);
    expect(scoped.nodes[0].disabled).toBe(true);
    expect(scoped.nodes[1].level).toBe(2);
    const shallow = normalizeAxTree(raw, null, 20, 1);
    expect(shallow.nodes.map((node) => node.name)).toEqual(['Page']);
    expect(shallow.truncated).toBe(true);
    const capped = normalizeAxTree(raw, null, 2, 20);
    expect(capped.nodes).toHaveLength(2);
    expect(capped.truncated).toBe(true);
  });

  it('returns normalized accessibility nodes and maps debugger failures to CAPABILITY_UNAVAILABLE', async () => {
    const deps = makeDeps({
      sendCdpCommand: vi.fn(async (_tab: any, method: string) =>
        method === 'Accessibility.getFullAXTree'
          ? { nodes: [{ nodeId: '1', role: { value: 'button' }, name: { value: 'OK' } }] }
          : {},
      ),
    });
    const result = await createExpandedHandlers(deps).page_accessibility({ max_nodes: 5, max_depth: 3 });
    expect(result).toEqual({
      url: 'https://example.com/page',
      count: 1,
      truncated: false,
      nodes: [{ role: 'button', name: 'OK', depth: 0 }],
    });
    const unavailable = createExpandedHandlers(
      makeDeps({
        sendCdpCommand: vi.fn(async () => {
          throw new Error('not attached to target');
        }),
      }),
    );
    await expectToolError(unavailable.page_accessibility({}), 'CAPABILITY_UNAVAILABLE');
  });
});

describe('tab window and cookie handlers', () => {
  it('groups and duplicates tabs with string ids', async () => {
    const group = vi.fn(async () => 12);
    const update = vi.fn(async () => ({}));
    const duplicate = vi.fn(async () => ({
      id: 9,
      index: 3,
      active: false,
      url: 'https://example.com/copy',
      title: 'Copy',
    }));
    (globalThis as any).chrome = {
      tabs: {
        get: vi.fn(async (id: number) => ({ id, windowId: 2 })),
        group,
        duplicate,
      },
      tabGroups: { update },
    };
    const handlers = createExpandedHandlers(makeDeps());
    expect(await handlers.tab_group({ tab_ids: ['1', '2'], title: 'Work', color: 'red' })).toEqual({
      group_id: '12',
      tab_ids: ['1', '2'],
      title: 'Work',
      color: 'red',
      window_id: '2',
    });
    expect(group).toHaveBeenCalledWith({ tabIds: [1, 2] });
    expect(update).toHaveBeenCalledWith(12, { color: 'red', title: 'Work' });
    expect(await handlers.tab_duplicate({ tab_id: '4' })).toEqual({
      duplicated: true,
      source_tab_id: '4',
      tab: { tab_id: '9', index: 3, active: false, url: 'https://example.com/copy', title: 'Copy' },
    });
    (globalThis as any).chrome.tabs.get = vi.fn(async () => {
      throw new Error('gone');
    });
    await expectToolError(createExpandedHandlers(makeDeps()).tab_duplicate({ tab_id: '404' }), 'NOT_FOUND');
  });

  it('lists and creates windows with tab caps and strict URLs', async () => {
    const create = vi.fn(async () => ({
      id: 5,
      incognito: false,
      width: 800,
      height: 600,
      tabs: [{ id: 6, url: 'https://example.com/new' }],
    }));
    (globalThis as any).chrome = {
      windows: {
        getAll: vi.fn(async () => [
          {
            id: 1,
            focused: true,
            state: 'normal',
            type: 'normal',
            incognito: false,
            left: 0,
            top: 0,
            width: 800,
            height: 600,
            tabs: [{ id: 11, index: 0, active: true, url: 'https://a.test', title: 'A' }],
          },
          {
            id: 2,
            focused: false,
            state: 'normal',
            type: 'normal',
            incognito: true,
            left: 1,
            top: 2,
            width: 900,
            height: 700,
            tabs: [],
          },
        ]),
        create,
      },
      tabs: { query: vi.fn(async () => [{ windowId: 1 }, { windowId: 1 }]) },
    };
    const handlers = createExpandedHandlers(makeDeps());
    const list = await handlers.window_list({ include_tabs: true, max_windows: 1 });
    expect(list.count).toBe(1);
    expect(list.truncated).toBe(true);
    expect(list.windows[0]).toMatchObject({
      window_id: '1',
      focused: true,
      tab_count: 2,
      tabs: [{ tab_id: '11', active: true }],
    });
    const withoutTabs = await handlers.window_list({ include_tabs: false });
    expect(withoutTabs.windows[1].tabs).toBeUndefined();
    expect(await handlers.window_new({ url: 'https://example.com/new', width: 800, height: 600 })).toEqual({
      created: true,
      window_id: '5',
      tab_id: '6',
      url: 'https://example.com/new',
      incognito: false,
      width: 800,
      height: 600,
    });
    await expectToolError(handlers.window_new({ url: 'chrome://settings' }), 'BAD_ARGS');
  });

  it('sets and clears cookies without echoing cookie values', async () => {
    const send = vi.fn(async (_tab: any, method: string) => {
      if (method === 'Storage.getCookies') {
        return {
          cookies: [
            { name: 'sid', value: 'secret', domain: '.example.com', path: '/' },
            { name: 'other', value: 'nope', domain: 'elsewhere.test', path: '/' },
          ],
        };
      }
      return method === 'Network.setCookie' ? { success: true } : {};
    });
    const deps = makeDeps({ sendCdpCommand: send });
    const handlers = createExpandedHandlers(deps);
    const setResult = await handlers.cookie_set({
      name: 'sid',
      value: 'secret',
      url: 'https://example.com/app',
      secure: true,
      http_only: true,
      same_site: 'strict',
    });
    expect(setResult).toEqual({
      set: true,
      name: 'sid',
      domain: 'example.com',
      path: '/',
      secure: true,
      http_only: true,
      same_site: 'strict',
      expires: 0,
      url: 'https://example.com/app',
    });
    expect(JSON.stringify(setResult)).not.toContain('secret');
    expect(send).toHaveBeenCalledWith(
      7,
      'Network.setCookie',
      expect.objectContaining({
        name: 'sid',
        value: 'secret',
        url: 'https://example.com/app',
        secure: true,
        httpOnly: true,
        sameSite: 'Strict',
      }),
    );
    const clearResult = await handlers.cookie_clear({ domain: 'example.com' });
    expect(clearResult).toEqual({ cleared: 1, names: ['sid'], domain: 'example.com' });
    expect(send).toHaveBeenCalledWith(7, 'Network.deleteCookies', { name: 'sid', domain: '.example.com', path: '/' });
    expect(deps.resolveTab).toHaveBeenCalledTimes(2);
    await expectToolError(handlers.cookie_set({ name: 'x', value: 'y', url: 'ftp://example.com' }), 'BAD_ARGS');
    const blankTab = createExpandedHandlers(
      makeDeps({ resolveTab: vi.fn(async () => ({ id: 7, url: 'about:blank' })) }),
    );
    await expectToolError(blankTab.cookie_clear({}), 'BAD_ARGS');
  });
});
