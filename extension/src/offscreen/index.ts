/**
 * Offscreen document host.
 *
 * MV3 service workers are suspended between commands, so this document owns the
 * single daemon WebSocket, the keepalive, the heartbeat and reconnect backoff.
 * The service worker remains the protocol and state owner; this file only moves
 * frames and reports transport state. It never touches chrome.storage: the
 * worker owns settings and forwards corvus.connect with the current URL.
 */

import { DEFAULT_DAEMON_URL, normalizeDaemonUrl, readDaemonUrl } from '../shared/storage/daemon-url';

const PROTOCOL_VERSION = 1;
const BACKOFF_MIN_MS = 250;
const BACKOFF_MAX_MS = 8000;
const SW_KEEPALIVE_MS = 20000;
const IDLE_PING_MS = 45000;
const DEAD_SOCKET_MS = 180000;
const HEALTH_CHECK_MS = 5000;
const HELLO_TIMEOUT_MS = 5000;
const CROCKFORD = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';

type ConnectionStatus = 'connecting' | 'connected' | 'disconnected';

let socket: WebSocket | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let keepaliveTimer: ReturnType<typeof setInterval> | null = null;
let healthTimer: ReturnType<typeof setInterval> | null = null;
let attempt = 0;
let lastFrameAt = 0;
let lastPingAt = 0;
let lastStatus: ConnectionStatus = 'disconnected';
let lastError: string | null = null;
let helloInFlight = false;
let currentUrl = DEFAULT_DAEMON_URL;

function now(): number {
  return Date.now();
}

function log(...args: unknown[]): void {
  console.debug('[corvus:offscreen]', ...args);
}

function describeError(error: unknown): string {
  if (!error) return 'unknown error';
  if (typeof error === 'string') return error;
  if (error instanceof Error && error.message) return error.message;
  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}

function makeId(): string {
  let time = now();
  let out = '';
  for (let index = 0; index < 10; index += 1) {
    out = CROCKFORD[time % 32] + out;
    time = Math.floor(time / 32);
  }
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  for (const byte of bytes) out += CROCKFORD[byte % 32];
  return out;
}

function runtimeSend(message: Record<string, unknown>): Promise<unknown> {
  try {
    const result = chrome.runtime.sendMessage(message);
    return Promise.resolve(result as Promise<unknown> | undefined).catch(() => undefined);
  } catch (error) {
    log('message to service worker failed', describeError(error));
    return Promise.resolve(undefined);
  }
}

function post(message: Record<string, unknown>): void {
  void runtimeSend({ source: 'corvus.offscreen', ...message });
}

function reportStatus(status: ConnectionStatus, error: string | null): void {
  lastStatus = status;
  lastError = error || null;
  post({ type: 'status', status, lastError, url: currentUrl, at: now() });
  log('status', status, lastError || '');
}

function clearReconnectTimer(): void {
  if (reconnectTimer !== null) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
}

function nextDelayMs(): number {
  const exponent = Math.min(attempt, 12);
  const base = Math.min(BACKOFF_MAX_MS, BACKOFF_MIN_MS * 2 ** exponent);
  const jitter = 0.75 + Math.random() * 0.5;
  return Math.max(BACKOFF_MIN_MS, Math.min(BACKOFF_MAX_MS, Math.round(base * jitter)));
}

function scheduleReconnect(reason: string): void {
  if (reconnectTimer !== null) return;
  const delay = nextDelayMs();
  attempt += 1;
  log('reconnect scheduled in ' + delay + ' ms (' + reason + ')');
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    connect('backoff');
  }, delay);
}

function sendFrame(frame: unknown): boolean {
  if (!socket || socket.readyState !== WebSocket.OPEN) {
    log('dropping outbound frame: socket is not open');
    return false;
  }
  try {
    socket.send(JSON.stringify(frame));
    return true;
  } catch (error) {
    log('socket send failed', describeError(error));
    return false;
  }
}

function connect(reason?: string, url?: string): void {
  if (typeof url === 'string' && url.trim()) {
    const normalized = normalizeDaemonUrl(url);
    if (!normalized) {
      log('ignoring invalid daemon URL', url);
      return;
    }
    if (normalized !== currentUrl) {
      currentUrl = normalized;
      clearReconnectTimer();
      attempt = 0;
      if (socket) {
        const previous = socket;
        socket = null;
        try {
          previous.close(4001, 'daemon URL changed');
        } catch (error) {
          log('old socket close failed', describeError(error));
        }
      }
    }
  }

  const state = socket ? socket.readyState : WebSocket.CLOSED;
  if (state === WebSocket.OPEN || state === WebSocket.CONNECTING) return;

  clearReconnectTimer();
  reportStatus('connecting', null);

  let next: WebSocket;
  try {
    next = new WebSocket(currentUrl);
  } catch (error) {
    reportStatus('disconnected', describeError(error));
    scheduleReconnect('constructor_failed');
    return;
  }

  socket = next;

  next.addEventListener('open', () => {
    attempt = 0;
    lastFrameAt = now();
    lastPingAt = 0;
    reportStatus('connected', null);
    post({ type: 'socket_open', at: now() });
    startTimers();
    void sendHello();
  });

  next.addEventListener('message', (event: MessageEvent) => {
    onSocketMessage(event);
  });

  next.addEventListener('error', () => {
    log('socket error');
  });

  next.addEventListener('close', (event: CloseEvent) => {
    onSocketClosed(event, next);
  });

  log('dialing', currentUrl, '(' + (reason || 'manual') + ')');
}

function onSocketMessage(event: MessageEvent): void {
  lastFrameAt = now();

  let frame: unknown;
  try {
    frame = JSON.parse(String(event.data));
  } catch {
    log('discarding frame that is not JSON');
    return;
  }
  if (!frame || typeof frame !== 'object') return;

  const record = frame as Record<string, unknown>;
  if (record.kind === 'ping') {
    sendFrame({ v: PROTOCOL_VERSION, id: String(record.id || makeId()), kind: 'pong' });
    return;
  }
  if (record.kind === 'pong') return;

  if (record.kind === 'hello_ack' || record.kind === 'command' || record.kind === 'event') {
    post({ type: 'frame', frame });
    return;
  }

  log('ignoring inbound frame kind', record.kind);
}

function onSocketClosed(event: CloseEvent, closedSocket: WebSocket): void {
  if (closedSocket !== socket) return;
  const code = typeof event.code === 'number' ? event.code : 0;
  const reason = event.reason ? String(event.reason) : '';
  const detail = code === 0 && !reason ? 'closed' : 'code=' + code + (reason ? ' reason=' + reason : '');
  socket = null;
  stopTimers();
  reportStatus('disconnected', detail);
  scheduleReconnect(detail);
}

async function sendHello(): Promise<void> {
  if (helloInFlight || !socket || socket.readyState !== WebSocket.OPEN) return;
  helloInFlight = true;

  let frame: Record<string, unknown> | null = null;
  try {
    const reply = await Promise.race([
      runtimeSend({ source: 'corvus.offscreen', type: 'hello_request' }),
      new Promise<null>((resolve) => setTimeout(() => resolve(null), HELLO_TIMEOUT_MS)),
    ]);
    if (reply && typeof reply === 'object' && 'frame' in reply) {
      const candidate = (reply as { frame?: unknown }).frame;
      if (candidate && typeof candidate === 'object') frame = candidate as Record<string, unknown>;
    }
  } catch (error) {
    log('hello request failed', describeError(error));
  }

  helloInFlight = false;

  if (!frame) {
    const manifest = chrome.runtime.getManifest();
    frame = {
      v: PROTOCOL_VERSION,
      id: makeId(),
      kind: 'hello',
      browser: 'unknown:default',
      args: {
        profile: 'default',
        version: manifest.version,
        capabilities: ['dom', 'scripting', 'debugger'],
      },
    };
    log('service worker did not answer; sending a minimal hello');
  }

  if (sendFrame(frame)) log('hello sent', frame.id);
}

function startTimers(): void {
  stopTimers();

  keepaliveTimer = setInterval(() => {
    post({ type: 'keepalive', at: now() });
  }, SW_KEEPALIVE_MS);

  healthTimer = setInterval(() => {
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    const idleFor = now() - lastFrameAt;

    if (idleFor >= DEAD_SOCKET_MS) {
      log('socket silent for ' + idleFor + ' ms; forcing a reconnect');
      try {
        socket.close(4000, 'heartbeat timeout');
      } catch (error) {
        log('socket close failed', describeError(error));
      }
      return;
    }

    if (idleFor >= IDLE_PING_MS && now() - lastPingAt >= IDLE_PING_MS) {
      lastPingAt = now();
      sendFrame({ v: PROTOCOL_VERSION, id: makeId(), kind: 'ping' });
    }
  }, HEALTH_CHECK_MS);
}

function stopTimers(): void {
  if (keepaliveTimer !== null) {
    clearInterval(keepaliveTimer);
    keepaliveTimer = null;
  }
  if (healthTimer !== null) {
    clearInterval(healthTimer);
    healthTimer = null;
  }
}

interface OffscreenRequest {
  target?: string;
  type?: string;
  frame?: unknown;
  url?: string;
  reason?: string;
}

chrome.runtime.onMessage.addListener((message: unknown, _sender, sendResponse) => {
  if (!message || typeof message !== 'object' || (message as OffscreenRequest).target !== 'offscreen') return false;
  const request = message as OffscreenRequest;

  switch (request.type) {
    case 'corvus.send':
      sendResponse({ ok: sendFrame(request.frame) });
      return false;
    case 'corvus.connect':
      connect(request.reason || 'service worker', request.url);
      sendResponse({ ok: true, status: lastStatus, url: currentUrl });
      return false;
    case 'corvus.status_request':
      sendResponse({ ok: true, status: lastStatus, lastError, attempt, url: currentUrl });
      return false;
    default:
      sendResponse({ ok: false, error: 'unknown offscreen message' });
      return false;
  }
});

self.addEventListener('online', () => {
  log('network is online; dialing immediately');
  attempt = 0;
  clearReconnectTimer();
  connect('network_online');
});

self.addEventListener('offline', () => {
  log('network is offline');
});

async function boot(): Promise<void> {
  try {
    currentUrl = await readDaemonUrl();
  } catch (error) {
    log('daemon URL read failed', describeError(error));
    currentUrl = DEFAULT_DAEMON_URL;
  }
  connect('offscreen_boot');
}

post({ type: 'keepalive', at: now() });
keepaliveTimer = setInterval(() => {
  post({ type: 'keepalive', at: now() });
}, SW_KEEPALIVE_MS);
void boot();

export {};
