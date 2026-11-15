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
    version: '1.4.0',
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
