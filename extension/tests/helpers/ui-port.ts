import { vi } from 'vitest';
import type { UiReply, UiState } from '../../src/shared/contracts/ui';

export function sampleState(overrides: Partial<UiState> = {}): UiState {
  return {
    status: 'connected',
    connected: true,
    browserId: 'chromium:test',
    daemonVersion: 'e2e-daemon',
    lastError: null,
    wsUrl: 'ws://127.0.0.1:29330',
    cdpEndpoint: 'auto',
    cdp: {
      available: true,
      endpoint: 'http://127.0.0.1:9222',
      webSocketDebuggerUrl: 'ws://127.0.0.1:9222/devtools/browser/test',
      browser: 'Chrome/157.0.0.0',
      browserFamily: 'chrome',
      expectedBrowserFamily: 'chrome',
      familyMatch: true,
      protocolVersion: '1.3',
      source: 'auto',
      reason: null,
      detected_at: 1,
    },
    version: '1.5.0',
    capabilities: ['dom', 'scripting', 'debugger', 'cdp'],
    debuggerChannel: { managed: true, attached_tabs: [] },
    ...overrides,
  };
}

interface FakePortHarness {
  messages: Array<{ id: string; type: string; payload: Record<string, unknown> }>;
  connect: ReturnType<typeof vi.fn>;
}

export function installFakeUiPort(
  handler?: (message: { id: string; type: string; payload: Record<string, unknown> }) => UiReply,
): FakePortHarness {
  const messages: FakePortHarness['messages'] = [];
  const listeners: Array<(message: unknown) => void> = [];

  const port = {
    name: 'corvus.ui',
    postMessage(message: { id: string; type: string; payload: Record<string, unknown> }): void {
      messages.push(message);
      const reply = handler
        ? handler(message)
        : message.type === 'corvus.daemon_save'
          ? { ok: true as const, state: sampleState({ wsUrl: String(message.payload.url) }) }
          : { ok: true as const, state: sampleState() };
      queueMicrotask(() => {
        for (const listener of listeners) listener({ id: message.id, ...reply });
      });
    },
    disconnect(): void {},
    onMessage: {
      addListener(listener: (message: unknown) => void): void {
        listeners.push(listener);
      },
      removeListener(): void {},
    },
    onDisconnect: {
      addListener(): void {},
      removeListener(): void {},
    },
  } as unknown as chrome.runtime.Port;

  const connect = vi.fn(() => port);
  vi.stubGlobal('chrome', { runtime: { connect, openOptionsPage: vi.fn(async () => undefined) } });
  return { messages, connect };
}

interface FakeStorageHarness {
  values: Record<string, unknown>;
  set: ReturnType<typeof vi.fn>;
  get: ReturnType<typeof vi.fn>;
  remove: ReturnType<typeof vi.fn>;
}

/**
 * Attach an in-memory chrome.storage.local to the chrome object installed by
 * installFakeUiPort. Keeps UI tests independent of a real extension profile.
 */
export function installFakeChromeStorage(initial: Record<string, unknown> = {}): FakeStorageHarness {
  const values: Record<string, unknown> = { ...initial };
  const get = vi.fn(async (keys?: string | string[] | Record<string, unknown> | null) => {
    if (typeof keys === 'string') return keys in values ? { [keys]: values[keys] } : {};
    if (Array.isArray(keys)) {
      const result: Record<string, unknown> = {};
      for (const key of keys) if (key in values) result[key] = values[key];
      return result;
    }
    if (keys && typeof keys === 'object') {
      const result: Record<string, unknown> = {};
      for (const key of Object.keys(keys)) result[key] = key in values ? values[key] : keys[key];
      return result;
    }
    return { ...values };
  });
  const set = vi.fn(async (items: Record<string, unknown>) => {
    Object.assign(values, items);
  });
  const remove = vi.fn(async (keys: string | string[]) => {
    for (const key of Array.isArray(keys) ? keys : [keys]) delete values[key];
  });
  const chromeGlobal = (globalThis as { chrome?: Record<string, unknown> }).chrome;
  if (!chromeGlobal) throw new Error('installFakeUiPort must run before installFakeChromeStorage');
  chromeGlobal.storage = { local: { get, set, remove } };
  return { values, set, get, remove };
}
