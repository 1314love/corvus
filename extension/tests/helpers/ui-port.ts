import { vi } from 'vitest';
import type { ProviderUiState, UiReply } from '../../src/shared/contracts/ui';

export function sampleState(overrides: Partial<ProviderUiState> = {}): ProviderUiState {
  return {
    status: 'connected',
    connected: true,
    base_url: 'ws://127.0.0.1:29330',
    token_saved: true,
    provider_id: 'provider-test',
    server_id: 'server-test',
    session_id: 's-test',
    browser: { family: 'Chrome', version: '157.0.0.0', profile: 'default' },
    last_error: null,
    version: '1.7.0',
    capabilities: [
      { id: 'system.info', version: 1, actions: ['info'] },
      { id: 'tabs', version: 1, actions: ['list', 'open', 'close'] },
    ],
    ...overrides,
  };
}

interface FakePortHarness {
  messages: Array<{ id: string; type: string; payload: Record<string, unknown> }>;
  connect: ReturnType<typeof vi.fn>;
}

let activePortDisconnect: (() => void) | null = null;

export function disconnectFakeUiPort(): void {
  const disconnect = activePortDisconnect;
  activePortDisconnect = null;
  disconnect?.();
}

export function installFakeUiPort(
  handler?: (message: { id: string; type: string; payload: Record<string, unknown> }) => UiReply,
): FakePortHarness {
  // The UI client is a module singleton. Disconnect the previous fake port so
  // the next mount creates a fresh port bound to this test's handler.
  activePortDisconnect?.();

  const messages: FakePortHarness['messages'] = [];
  const listeners: Array<(message: unknown) => void> = [];
  const disconnectListeners: Array<() => void> = [];

  const port = {
    name: 'corvus.ui',
    postMessage(message: { id: string; type: string; payload: Record<string, unknown> }): void {
      messages.push(message);
      const reply = handler
        ? handler(message)
        : message.type === 'corvus.save_and_connect'
          ? {
              ok: true as const,
              state: sampleState({
                base_url: String(message.payload.base_url),
                token_saved: Boolean(message.payload.token),
              }),
            }
          : { ok: true as const, state: sampleState() };
      queueMicrotask(() => {
        for (const listener of listeners) listener({ id: message.id, ...reply });
      });
    },
    disconnect(): void {
      for (const listener of disconnectListeners.splice(0)) listener();
    },
    onMessage: {
      addListener(listener: (message: unknown) => void): void {
        listeners.push(listener);
      },
      removeListener(): void {},
    },
    onDisconnect: {
      addListener(listener: () => void): void {
        disconnectListeners.push(listener);
      },
      removeListener(): void {},
    },
  } as unknown as chrome.runtime.Port;

  const connect = vi.fn(() => port);
  vi.stubGlobal('chrome', { runtime: { connect, openOptionsPage: vi.fn(async () => undefined) } });
  activePortDisconnect = () => port.disconnect();
  return { messages, connect };
}

interface FakeStorageHarness {
  values: Record<string, unknown>;
  set: ReturnType<typeof vi.fn>;
  get: ReturnType<typeof vi.fn>;
  remove: ReturnType<typeof vi.fn>;
}

/** Attach an in-memory chrome.storage.local to the chrome object installed by installFakeUiPort. */
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
