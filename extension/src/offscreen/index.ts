/**
 * P2 provider-only offscreen host.
 *
 * The offscreen document owns exactly one encrypted provider socket. It reads
 * the loopback base URL and pairing token from chrome.storage.local, performs
 * the P1 handshake and forwards capability actions to the service worker. No
 * plaintext bridge, keepalive protocol or debugger endpoint exists here.
 */
import { providerCapabilities } from '../background/provider-actions';
import {
  DAEMON_URL_STORAGE_KEY,
  PAIRING_TOKEN_STORAGE_KEY,
  providerBaseFromStorageValue,
  providerEndpointFromBase,
} from '../shared/provider/endpoint';
import { normalizePairingToken } from '../shared/provider/token';
import { readStorageValue } from '../shared/storage/adapter';
import {
  ProviderSocket,
  ProviderSocketError,
  type ProviderBrowserInfo,
  type ProviderConnectionInfo,
  type ProviderSocketState,
} from './provider-socket';

type HostStatus = 'pairing_required' | 'connecting' | 'connected' | 'disconnected' | 'error';

interface HostState {
  status: HostStatus;
  error: { code: string; message: string } | null;
  base_url: string;
  provider_id: string | null;
  server_id: string | null;
  session_id: string | null;
  browser: ProviderBrowserInfo | null;
}

const BACKOFF_MIN_MS = 500;
const BACKOFF_MAX_MS = 30000;
const DEFAULT_BASE = 'ws://127.0.0.1:7330';

let socket: ProviderSocket | null = null;
let timer: ReturnType<typeof setTimeout> | null = null;
let attempt = 0;
let manualStop = false;
let connecting = false;
let state: HostState = {
  status: 'disconnected',
  error: null,
  base_url: DEFAULT_BASE,
  provider_id: null,
  server_id: null,
  session_id: null,
  browser: null,
};

function extensionVersion(): string {
  try {
    if (typeof chrome.runtime.getManifest === 'function') {
      const manifest = chrome.runtime.getManifest();
      if (manifest && typeof manifest.version === 'string') return manifest.version;
    }
  } catch {
    // The offscreen document may not expose the manifest API.
  }
  return '2.0.0';
}

function browserInfo(): ProviderBrowserInfo {
  const userAgent = typeof navigator === 'undefined' ? '' : navigator.userAgent;
  const family = /Edg\//.test(userAgent) ? 'Edge' : /Chrome\//.test(userAgent) ? 'Chrome' : 'Chromium';
  const match = /(?:Chrome|Edg|Firefox)\/([0-9.]+)/.exec(userAgent);
  return { family, version: match ? match[1] : 'unknown', profile: 'default' };
}

function messageFor(error: unknown): { code: string; message: string } {
  const code = error instanceof ProviderSocketError ? error.code : 'INTERNAL';
  switch (code) {
    case 'AUTHENTICATION':
      return { code, message: 'Pairing was rejected. Check the token.' };
    case 'VERSION':
      return { code, message: 'The local service did not accept the protocol version.' };
    case 'REPLAY':
    case 'MALFORMED':
      return { code, message: 'The encrypted connection failed a security check.' };
    case 'HANDSHAKE_TIMEOUT':
    case 'TIMEOUT':
      return { code, message: 'The local service did not finish connecting in time.' };
    case 'SIZE':
      return { code, message: 'The connection exceeded its size limit.' };
    case 'BAD_ARGS':
      return { code, message: 'The local URL is invalid.' };
    default:
      return { code, message: 'The local service is unavailable.' };
  }
}

function publish(next: Partial<HostState>): void {
  state = {
    ...state,
    ...next,
    base_url: next.base_url ?? state.base_url,
  };
  try {
    void chrome.runtime.sendMessage({
      source: 'corvus.offscreen',
      type: 'provider.state',
      status: state.status,
      error: state.error,
      base_url: state.base_url,
      provider_id: state.provider_id,
      server_id: state.server_id,
      session_id: state.session_id,
      browser: state.browser,
    });
  } catch {
    // The service worker may be restarting.
  }
}

function clearTimer(): void {
  if (timer !== null) {
    clearTimeout(timer);
    timer = null;
  }
}

function nextDelay(): number {
  const base = Math.min(BACKOFF_MAX_MS, BACKOFF_MIN_MS * 2 ** Math.min(attempt, 8));
  return Math.max(BACKOFF_MIN_MS, Math.round(base * (0.8 + Math.random() * 0.4)));
}

function scheduleReconnect(): void {
  if (manualStop || timer !== null) return;
  const delay = nextDelay();
  attempt += 1;
  timer = setTimeout(() => {
    timer = null;
    void connectNow();
  }, delay);
}

async function forwardAction(action: string, args: Record<string, unknown>): Promise<unknown> {
  let reply: unknown;
  try {
    reply = await chrome.runtime.sendMessage({
      source: 'corvus.offscreen',
      type: 'provider.action',
      action,
      args,
    });
  } catch {
    throw new ProviderSocketError('PROVIDER_LOST', 'the service worker did not answer');
  }
  if (reply && typeof reply === 'object' && (reply as Record<string, unknown>).ok === true) {
    return (reply as Record<string, unknown>).value;
  }
  const error = reply && typeof reply === 'object' ? (reply as Record<string, unknown>).error : null;
  if (error && typeof error === 'object') {
    const record = error as Record<string, unknown>;
    throw new ProviderSocketError(
      typeof record.code === 'string' ? record.code : 'INTERNAL',
      typeof record.message === 'string' ? record.message : 'provider action failed',
    );
  }
  throw new ProviderSocketError('PROVIDER_LOST', 'the service worker did not answer');
}

async function connectNow(baseHint?: string, tokenHint?: string | null): Promise<void> {
  if (connecting) return;
  clearTimer();
  connecting = true;
  const previous = socket;
  socket = null;
  if (previous) {
    try {
      previous.close(1000, 'reconnect');
    } catch {
      // The old transport is already gone.
    }
  }

  let base = typeof baseHint === 'string' ? providerBaseFromStorageValue(baseHint) : DEFAULT_BASE;
  let token: string | null = tokenHint !== undefined ? normalizePairingToken(tokenHint) : null;
  if (baseHint === undefined || tokenHint === undefined) {
    try {
      const storedBase = await readStorageValue(DAEMON_URL_STORAGE_KEY);
      const storedToken = await readStorageValue(PAIRING_TOKEN_STORAGE_KEY);
      if (baseHint === undefined) base = providerBaseFromStorageValue(storedBase);
      if (tokenHint === undefined) token = normalizePairingToken(storedToken);
    } catch {
      // Storage is unavailable; report a clean pairing state below.
    }
  }
  state.base_url = base;
  if (!token) {
    connecting = false;
    publish({
      status: 'pairing_required',
      error: null,
      provider_id: null,
      server_id: null,
      session_id: null,
      browser: null,
    });
    return;
  }
  const endpoint = providerEndpointFromBase(base);
  if (!endpoint) {
    connecting = false;
    publish({ status: 'error', error: { code: 'BAD_ARGS', message: 'The local URL is invalid.' } });
    return;
  }

  publish({ status: 'connecting', error: null, provider_id: null, server_id: null, session_id: null, browser: null });
  let current: ProviderSocket;
  try {
    current = new ProviderSocket({
      url: endpoint,
      token,
      browser: browserInfo(),
      extensionVersion: extensionVersion(),
      actions: {
        capabilities: providerCapabilities(),
        invokeAction: forwardAction,
      },
      onClose: (error) => {
        if (socket !== current) return;
        socket = null;
        publish({
          status: 'disconnected',
          error: error ? messageFor(error) : null,
          provider_id: null,
          server_id: null,
          session_id: null,
          browser: null,
        });
        scheduleReconnect();
      },
    });
  } catch (failure) {
    connecting = false;
    publish({
      status: 'error',
      error: messageFor(failure),
      provider_id: null,
      server_id: null,
      session_id: null,
      browser: null,
    });
    scheduleReconnect();
    return;
  }
  socket = current;
  try {
    const connection: ProviderConnectionInfo = await current.connect();
    attempt = 0;
    connecting = false;
    if (socket === current) {
      publish({
        status: 'connected',
        error: null,
        provider_id: connection.provider_id,
        server_id: connection.server_id,
        session_id: connection.session_id,
        browser: connection.browser,
      });
    }
  } catch (error) {
    connecting = false;
    if (socket === current) {
      socket = null;
      publish({
        status: 'error',
        error: messageFor(error),
        provider_id: null,
        server_id: null,
        session_id: null,
        browser: null,
      });
    }
    scheduleReconnect();
  }
}

function disconnect(): void {
  manualStop = true;
  clearTimer();
  attempt = 0;
  const current = socket;
  socket = null;
  if (current) {
    try {
      current.close(1000, 'disconnect');
    } catch {
      // The transport is already closed.
    }
  }
  publish({ status: 'disconnected', error: null, provider_id: null, server_id: null, session_id: null, browser: null });
}

function status(): ProviderSocketState | 'pairing_required' {
  return socket ? socket.state : state.status === 'pairing_required' ? 'pairing_required' : 'closed';
}

chrome.runtime.onMessage.addListener((message: unknown, _sender, sendResponse) => {
  if (!message || typeof message !== 'object' || (message as Record<string, unknown>).target !== 'offscreen')
    return false;
  const type = (message as Record<string, unknown>).type;
  if (type === 'provider.connect') {
    manualStop = false;
    attempt = 0;
    const base =
      typeof (message as Record<string, unknown>).base_url === 'string'
        ? ((message as Record<string, unknown>).base_url as string)
        : undefined;
    const token =
      typeof (message as Record<string, unknown>).token === 'string'
        ? ((message as Record<string, unknown>).token as string)
        : undefined;
    void connectNow(base, token)
      .then(() => sendResponse({ ok: true, status: status() }))
      .catch((failure) => sendResponse({ ok: false, status: status(), error: String(failure) }));
    return true;
  }
  if (type === 'provider.disconnect') {
    disconnect();
    sendResponse({ ok: true, status: status() });
    return false;
  }
  if (type === 'provider.status_request') {
    sendResponse({ ok: true, state });
    return false;
  }
  sendResponse({ ok: false, error: 'unknown offscreen message' });
  return false;
});

self.addEventListener('online', () => {
  if (!manualStop) {
    attempt = 0;
    void connectNow();
  }
});

void (async () => {
  try {
    state.base_url = providerBaseFromStorageValue(await readStorageValue(DAEMON_URL_STORAGE_KEY));
  } catch {
    state.base_url = DEFAULT_BASE;
  }
  const token = normalizePairingToken(await readStorageValue(PAIRING_TOKEN_STORAGE_KEY).catch(() => undefined));
  if (token) {
    await connectNow();
  } else {
    publish({ status: 'pairing_required', error: null });
  }
})();

export {};
