import { afterEach, describe, expect, it, vi } from 'vitest';
import { ProviderSocket, ProviderSocketError, type ProviderSocketOptions } from '../../src/offscreen/provider-socket';
import { encodeBase64Url } from '../../src/shared/provider/protocol';
import { TestGateway } from './provider-gateway';

const URL = 'ws://127.0.0.1:17330/provider/v2';
const sockets: ProviderSocket[] = [];
const gateways: TestGateway[] = [];

function openSocket(gateway: TestGateway, overrides: Partial<ProviderSocketOptions> = {}) {
  const socket = new ProviderSocket({
    url: URL,
    token: gateway.token,
    socketFactory: () => {
      queueMicrotask(() => gateway.pair.open());
      return gateway.pair.client;
    },
    handshakeTimeoutMs: 1500,
    ...overrides,
  });
  sockets.push(socket);
  const connecting = socket.connect();
  return { socket, connecting };
}

async function waitFor(predicate: () => boolean, timeoutMs: number = 2000): Promise<void> {
  const started = Date.now();
  while (!predicate()) {
    if (Date.now() - started > timeoutMs) throw new Error('condition was not reached in time');
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
}

afterEach(() => {
  for (const socket of sockets.splice(0)) {
    try {
      socket.close();
    } catch {
      // Already closed.
    }
  }
  for (const gateway of gateways.splice(0)) gateway.dispose();
});

describe('encrypted provider socket', () => {
  it('performs a real handshake and round-trips system.info and tabs.read in both directions', async () => {
    const gateway = await TestGateway.create();
    gateways.push(gateway);
    const extensionInfo = {
      browser_family: 'Chrome',
      browser_version: '157.0.0.0',
      extension_version: '2.0.0',
      capabilities: ['system.info', 'tabs.read'],
    };
    const extensionTabs = [
      {
        tab_id: '11',
        window_id: 2,
        title: 'Extension tab',
        url: 'https://extension.example/',
        active: true,
      },
    ];
    const { socket, connecting } = openSocket(gateway, {
      browser: { family: 'Chrome', version: '157.0.0.0', profile: 'test' },
      actions: { systemInfo: () => extensionInfo, tabsList: () => extensionTabs },
    });

    const connection = await connecting;
    expect(socket.state).toBe('ready');
    expect(connection.server_id).toBe(gateway.serverId);
    expect(connection.session_id).toBe(gateway.sessionId);
    expect(connection.provider_id).toMatch(/^provider-[0-9a-f-]{36}$/);
    expect(gateway.manifest?.provider_id).toBe(connection.provider_id);
    expect(gateway.manifest?.browser).toEqual({ family: 'Chrome', version: '157.0.0.0', profile: 'test' });

    await expect(socket.requestSystemInfo()).resolves.toEqual(gateway.systemInfo);
    await expect(socket.requestTabsList()).resolves.toEqual(gateway.tabs);
    await expect(gateway.invoke('system.info.info')).resolves.toEqual(extensionInfo);
    await expect(gateway.invoke('tabs.read.list')).resolves.toEqual(extensionTabs);
  });

  it('rejects an invalid or missing pairing token before opening a socket', async () => {
    const gateway = await TestGateway.create();
    gateways.push(gateway);
    const socket = new ProviderSocket({
      url: URL,
      token: 'not-a-valid-token',
      socketFactory: () => gateway.pair.client,
      handshakeTimeoutMs: 200,
    });
    sockets.push(socket);
    const error = await socket.connect().catch((failure: unknown) => failure);
    expect(error).toBeInstanceOf(ProviderSocketError);
    expect((error as ProviderSocketError).code).toBe('AUTHENTICATION');
    expect((error as ProviderSocketError).closeCode).toBe(4401);
    expect(socket.state).toBe('closed');
  });

  it('closes with 4401 when the pairing proof uses the wrong 32-byte token', async () => {
    const gateway = await TestGateway.create();
    gateways.push(gateway);
    const wrongToken = Uint8Array.from({ length: 32 }, (_, index) => 200 + index);
    const { socket, connecting } = openSocket(gateway, { token: wrongToken });
    const error = await connecting.catch((failure: unknown) => failure);
    expect(error).toBeInstanceOf(ProviderSocketError);
    expect((error as ProviderSocketError).code).toBe('AUTHENTICATION');
    expect((error as ProviderSocketError).closeCode).toBe(4401);
    expect(socket.lastError?.closeCode).toBe(4401);
  });

  it('closes with 4403 for a protocol version mismatch', async () => {
    const gateway = await TestGateway.create({ protocol: 3 });
    gateways.push(gateway);
    const { socket, connecting } = openSocket(gateway);
    const error = await connecting.catch((failure: unknown) => failure);
    expect(error).toBeInstanceOf(ProviderSocketError);
    expect((error as ProviderSocketError).code).toBe('VERSION');
    expect((error as ProviderSocketError).closeCode).toBe(4403);
    expect(socket.lastError?.closeCode).toBe(4403);
  });

  it('closes with 4401 when the server confirmation proof is wrong', async () => {
    const gateway = await TestGateway.create({ wrongConfirm: true });
    gateways.push(gateway);
    const { socket, connecting } = openSocket(gateway);
    const error = await connecting.catch((failure: unknown) => failure);
    expect(error).toBeInstanceOf(ProviderSocketError);
    expect((error as ProviderSocketError).code).toBe('AUTHENTICATION');
    expect((error as ProviderSocketError).closeCode).toBe(4401);
    expect(socket.lastError?.closeCode).toBe(4401);
  });

  it('closes with 4404 when a post-handshake frame is tampered', async () => {
    const gateway = await TestGateway.create();
    gateways.push(gateway);
    const { socket, connecting } = openSocket(gateway);
    await connecting;
    await gateway.sendTamperedEvent();
    await waitFor(() => socket.state === 'closed');
    expect(socket.lastError?.code).toBe('MALFORMED');
    expect(socket.lastError?.closeCode).toBe(4404);
  });

  it('closes with 4402 when a frame is replayed', async () => {
    const gateway = await TestGateway.create();
    gateways.push(gateway);
    const { socket, connecting } = openSocket(gateway);
    await connecting;
    await gateway.sendEvent({ replayed: true });
    await gateway.replayLastFrame();
    await waitFor(() => socket.state === 'closed');
    expect(socket.lastError?.code).toBe('REPLAY');
    expect(socket.lastError?.closeCode).toBe(4402);
  });

  it('closes with 4402 when a frame sequence has a gap', async () => {
    const gateway = await TestGateway.create();
    gateways.push(gateway);
    const { socket, connecting } = openSocket(gateway);
    await connecting;
    await gateway.sendGapEvent();
    await waitFor(() => socket.state === 'closed');
    expect(socket.lastError?.code).toBe('REPLAY');
    expect(socket.lastError?.closeCode).toBe(4402);
  });

  it('closes with 4408 when the server never sends server_hello', async () => {
    const gateway = await TestGateway.create({ autoHello: false });
    gateways.push(gateway);
    const { socket, connecting } = openSocket(gateway, { handshakeTimeoutMs: 40 });
    const error = await connecting.catch((failure: unknown) => failure);
    expect(error).toBeInstanceOf(ProviderSocketError);
    expect((error as ProviderSocketError).code).toBe('HANDSHAKE_TIMEOUT');
    expect((error as ProviderSocketError).closeCode).toBe(4408);
    expect(socket.lastError?.closeCode).toBe(4408);
  });

  it('never puts the pairing token in errors or console output', async () => {
    const gateway = await TestGateway.create();
    gateways.push(gateway);
    const token = Uint8Array.from({ length: 32 }, (_, index) => 100 + index);
    const tokenText = encodeBase64Url(token);
    const consoleCalls: unknown[] = [];
    const spies = ['log', 'warn', 'error', 'debug'].map((method) =>
      vi.spyOn(console, method as 'log').mockImplementation((...args: unknown[]) => {
        consoleCalls.push(...args);
      }),
    );
    try {
      const { connecting } = openSocket(gateway, { token });
      const error = (await connecting.catch((failure: unknown) => failure)) as ProviderSocketError;
      const rendered = JSON.stringify({ code: error.code, message: error.message, closeCode: error.closeCode });
      expect(rendered).not.toContain(tokenText);
      expect(consoleCalls.join(' ')).not.toContain(tokenText);
    } finally {
      for (const spy of spies) spy.mockRestore();
    }
  });
});
