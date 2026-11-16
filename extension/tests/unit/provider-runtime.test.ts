import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

type RuntimeModule = typeof import('../../src/background/provider-runtime');

interface FakeChromeState {
  local: Map<string, unknown>;
  session: Map<string, unknown>;
  sent: Array<Record<string, unknown>>;
  contexts: Array<Record<string, unknown>>;
  createDocument: ReturnType<typeof vi.fn>;
  capture: ReturnType<typeof vi.fn>;
}

const state: FakeChromeState = {
  local: new Map(),
  session: new Map(),
  sent: [],
  contexts: [{ contextType: 'OFFSCREEN_DOCUMENT' }],
  createDocument: vi.fn(async () => undefined),
  capture: vi.fn(),
};

let runtime: RuntimeModule;

function area(map: Map<string, unknown>) {
  return {
    get: vi.fn(async (keys?: unknown) => {
      if (typeof keys === 'string') return map.has(keys) ? { [keys]: map.get(keys) } : {};
      if (Array.isArray(keys)) {
        const out: Record<string, unknown> = {};
        for (const key of keys) if (map.has(String(key))) out[String(key)] = map.get(String(key));
        return out;
      }
      return Object.fromEntries(map);
    }),
    set: vi.fn(async (items: Record<string, unknown>) => {
      for (const [key, value] of Object.entries(items)) map.set(key, value);
    }),
    remove: vi.fn(async (keys: string | string[]) => {
      for (const key of Array.isArray(keys) ? keys : [keys]) map.delete(key);
    }),
  };
}

const token = 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA';

beforeAll(async () => {
  const chromeStub = {
    storage: { local: area(state.local), session: area(state.session) },
    runtime: {
      getManifest: () => ({ version: '1.7.0' }),
      getURL: (path: string) => 'chrome-extension://corvus/' + path,
      getContexts: vi.fn(async () => state.contexts),
      sendMessage: vi.fn(async (message: Record<string, unknown>) => {
        state.sent.push(message);
        return { ok: true };
      }),
      onMessage: { addListener: vi.fn() },
      onStartup: { addListener: vi.fn() },
      onInstalled: { addListener: vi.fn() },
    },
    offscreen: { createDocument: state.createDocument },
    tabs: {
      get: vi.fn(async (id: number) => ({
        id,
        windowId: 3,
        url: 'https://example.com/',
        title: 'Example',
        active: true,
      })),
      query: vi.fn(async (info?: { active?: boolean }) =>
        info && info.active === true
          ? [{ id: 7, windowId: 3, url: 'https://example.com/', title: 'Example', active: true }]
          : [{ id: 7, windowId: 3, url: 'https://example.com/', title: 'Example', active: true }],
      ),
      create: vi.fn(async () => ({ id: 8, windowId: 3, url: 'https://example.com/', title: 'New', active: true })),
      remove: vi.fn(async () => undefined),
      update: vi.fn(async () => ({})),
      sendMessage: vi.fn(async () => ({ ok: true, value: {} })),
      captureVisibleTab: state.capture,
    },
    scripting: { executeScript: vi.fn(async () => [{ result: 'complete' }]) },
  };
  vi.stubGlobal('chrome', chromeStub);
  runtime = await import('../../src/background/provider-runtime');
});

beforeEach(() => {
  state.local.clear();
  state.session.clear();
  state.sent.length = 0;
  state.createDocument.mockClear();
  state.capture.mockReset();
  vi.clearAllMocks();
  // clearAllMocks also resets implementations on some mocks; restore the capture stub per test.
});

function pngDataUrl(width = 20, height = 10): string {
  const bytes = new Uint8Array(33);
  bytes.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], 0);
  const view = new DataView(bytes.buffer);
  view.setUint32(16, width, false);
  view.setUint32(20, height, false);
  return 'data:image/png;base64,' + Buffer.from(bytes).toString('base64');
}

function jpegDataUrl(width = 32, height = 16): string {
  const bytes = new Uint8Array([
    0xff,
    0xd8,
    0xff,
    0xc0,
    0x00,
    0x11,
    0x08,
    (height >> 8) & 0xff,
    height & 0xff,
    (width >> 8) & 0xff,
    width & 0xff,
    0x03,
    0x01,
    0x11,
    0x00,
    0x02,
    0x11,
    0x01,
    0x03,
    0x11,
    0x01,
  ]);
  return 'data:image/jpeg;base64,' + Buffer.from(bytes).toString('base64');
}

describe('provider UI state and pairing', () => {
  it('starts disconnected without a token and reports the default base', async () => {
    const reply = await runtime.handleProviderUiRequest({ kind: 'get_state' });
    expect(reply.ok).toBe(true);
    if (!reply.ok) return;
    expect(reply.state.status).toBe('disconnected');
    expect(reply.state.base_url).toBe('ws://127.0.0.1:7330');
    expect(reply.state.token_saved).toBe(false);
    expect(reply.state.version).toBe('1.7.0');
    expect(reply.state.capabilities.map((capability) => capability.id)).toEqual([
      'system.info',
      'tabs',
      'page',
      'input',
      'capture',
    ]);
  });

  it('rejects a non-loopback base and an invalid token before writing anything', async () => {
    expect(
      await runtime.handleProviderUiRequest({ kind: 'save_and_connect', base_url: 'https://example.com', token }),
    ).toEqual({
      ok: false,
      error: 'Enter a loopback ws:// URL, for example ws://127.0.0.1:7330.',
    });
    expect(
      await runtime.handleProviderUiRequest({
        kind: 'save_and_connect',
        base_url: 'ws://127.0.0.1:8123',
        token: 'short',
      }),
    ).toEqual({
      ok: false,
      error: 'Enter the 43-character pairing token.',
    });
    expect(state.local.size).toBe(0);
    expect(state.sent).toHaveLength(0);
  });

  it('stores the token and base, then asks the offscreen document to connect', async () => {
    const reply = await runtime.handleProviderUiRequest({
      kind: 'save_and_connect',
      base_url: 'ws://127.0.0.1:8123',
      token,
    });
    expect(reply.ok).toBe(true);
    if (!reply.ok) return;
    expect(reply.state.token_saved).toBe(true);
    expect(reply.state.base_url).toBe('ws://127.0.0.1:8123');
    expect(state.local.get('provider.pairingToken')).toBe(token);
    expect(state.local.get('corvus.daemonUrl')).toBe('ws://127.0.0.1:8123');
    expect(state.sent).toContainEqual({
      target: 'offscreen',
      type: 'provider.connect',
      base_url: 'ws://127.0.0.1:8123',
      token,
    });
  });

  it('refuses connect without a saved token and disconnects cleanly', async () => {
    expect(await runtime.handleProviderUiRequest({ kind: 'connect' })).toEqual({
      ok: false,
      error: 'Enter the pairing token before connecting.',
    });
    state.local.set('provider.pairingToken', token);
    state.session.set('provider.uiState', {
      status: 'connected',
      connected: true,
      provider_id: 'provider-test',
      server_id: 'server-test',
      session_id: 's-test',
    });
    const disconnected = await runtime.handleProviderUiRequest({ kind: 'disconnect' });
    expect(disconnected.ok).toBe(true);
    if (!disconnected.ok) return;
    expect(disconnected.state.status).toBe('disconnected');
    expect(disconnected.state.connected).toBe(false);
    expect(state.sent).toContainEqual({ target: 'offscreen', type: 'provider.disconnect' });
  });

  it('never returns the pairing token in state', async () => {
    state.local.set('provider.pairingToken', token);
    const reply = await runtime.handleProviderUiRequest({ kind: 'get_state' });
    expect(JSON.stringify(reply)).not.toContain(token);
  });
});

describe('provider screenshot format', () => {
  it('returns PNG data with decoded dimensions for the default format', async () => {
    state.capture.mockResolvedValue(pngDataUrl(20, 10));
    const value = (await runtime.handleProviderAction('capture.viewport', {})) as Record<string, unknown>;
    expect(value).toMatchObject({ tab_id: '7', format: 'png', mime_type: 'image/png', width: 20, height: 10 });
    expect(String(value.data)).toMatch(/^iVBOR/);
    expect(state.capture).toHaveBeenCalledWith(3, { format: 'png' });
  });

  it('passes jpeg quality through and reports jpeg dimensions', async () => {
    state.capture.mockResolvedValue(jpegDataUrl(32, 16));
    const value = (await runtime.handleProviderAction('capture.viewport', { format: 'jpeg', quality: 60 })) as Record<
      string,
      unknown
    >;
    expect(value).toMatchObject({ format: 'jpeg', mime_type: 'image/jpeg', width: 32, height: 16 });
    expect(state.capture).toHaveBeenCalledWith(3, { format: 'jpeg', quality: 60 });
  });

  it('accepts quality on png but ignores it, and rejects an empty capture', async () => {
    state.capture.mockResolvedValue(pngDataUrl(20, 10));
    const value = (await runtime.handleProviderAction('capture.viewport', {
      format: 'png',
      quality: 10,
    })) as Record<string, unknown>;
    expect(value).toMatchObject({ format: 'png', mime_type: 'image/png', width: 20, height: 10 });
    expect(state.capture).toHaveBeenCalledWith(3, { format: 'png' });
    state.capture.mockResolvedValue('data:image/png;base64,');
    await expect(runtime.handleProviderAction('capture.viewport', {})).rejects.toMatchObject({ code: 'INTERNAL' });
  });
});

describe('provider content results', () => {
  it('adds the resolved tab id to every page and input result, including an omitted tab', async () => {
    const sendMessage = chrome.tabs.sendMessage as unknown as ReturnType<typeof vi.fn>;
    sendMessage
      .mockResolvedValueOnce({
        ok: true,
        value: { url: 'https://example.com/', mode: 'text', content: 'Example', truncated: false },
      })
      .mockResolvedValueOnce({
        ok: true,
        value: { url: 'https://example.com/', nodes: [], truncated: false },
      })
      .mockResolvedValueOnce({ ok: true, value: { ref: '@e1', clicked: true } })
      .mockResolvedValueOnce({ ok: true, value: { ref: '@e2', typed_chars: 2, submitted: false } });

    await expect(runtime.handleProviderAction('page.read', { max_chars: 1000 })).resolves.toMatchObject({
      tab_id: '7',
      content: 'Example',
    });
    await expect(runtime.handleProviderAction('page.snapshot', { max_nodes: 100 })).resolves.toMatchObject({
      tab_id: '7',
      nodes: [],
    });
    await expect(runtime.handleProviderAction('input.click', { ref: '@e1' })).resolves.toMatchObject({
      tab_id: '7',
      clicked: true,
    });
    await expect(runtime.handleProviderAction('input.type', { ref: '@e2', text: 'hi' })).resolves.toMatchObject({
      tab_id: '7',
      typed_chars: 2,
    });
  });

  it('uses the explicit tab id in content action results', async () => {
    const sendMessage = chrome.tabs.sendMessage as unknown as ReturnType<typeof vi.fn>;
    sendMessage.mockResolvedValueOnce({
      ok: true,
      value: { url: 'https://example.com/9', mode: 'html', content: '<p>nine</p>', truncated: false },
    });
    await expect(runtime.handleProviderAction('page.read', { tab_id: '9', mode: 'html' })).resolves.toMatchObject({
      tab_id: '9',
      content: '<p>nine</p>',
    });
    expect(sendMessage).toHaveBeenCalledWith(
      9,
      expect.objectContaining({
        type: 'corvus.provider_action',
        action: 'page.read',
        args: expect.objectContaining({ mode: 'html', max_chars: 20000 }),
      }),
    );
  });
});
