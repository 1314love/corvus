'use strict';

/**
 * Corvus offscreen document.
 *
 * An MV3 service worker cannot hold a WebSocket reliably (it is suspended after
 * roughly thirty seconds of inactivity), so this document owns the single socket
 * to the daemon and relays envelopes in both directions:
 *
 *   daemon --WS--> offscreen --chrome.runtime.sendMessage--> service worker
 *   service worker --chrome.runtime.sendMessage--> offscreen --WS--> daemon
 *
 * The document also pokes the service worker every twenty seconds so the worker
 * is not torn down between commands, and it reconnects with exponential backoff
 * plus jitter whenever the socket closes.
 *
 * This file is plain JavaScript. No build step, no dependencies.
 */

(() => {
  const PROTOCOL_VERSION = 1;
  const WS_URL = 'ws://127.0.0.1:7330';

  const BACKOFF_MIN_MS = 250;
  const BACKOFF_MAX_MS = 8000;
  const SW_KEEPALIVE_MS = 20000;
  const IDLE_PING_MS = 45000;
  const DEAD_SOCKET_MS = 180000;
  const HEALTH_CHECK_MS = 5000;
  const HELLO_TIMEOUT_MS = 5000;

  const CROCKFORD = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';

  let socket = null;
  let reconnectTimer = null;
  let keepaliveTimer = null;
  let healthTimer = null;
  let attempt = 0;
  let lastFrameAt = 0;
  let lastPingAt = 0;
  let lastStatus = 'disconnected';
  let lastError = null;
  let helloInFlight = false;

  function now() {
    return Date.now();
  }

  function log() {
    const args = Array.prototype.slice.call(arguments);
    console.debug.apply(console, ['[corvus:offscreen]'].concat(args));
  }

  function describeError(error) {
    if (!error) return 'unknown error';
    if (typeof error === 'string') return error;
    if (error.message) return String(error.message);
    try {
      return JSON.stringify(error);
    } catch (_) {
      return String(error);
    }
  }

  /** ULID-shaped identifier: 10 time chars plus 16 random chars. */
  function makeId() {
    let time = now();
    let out = '';
    for (let i = 0; i < 10; i += 1) {
      out = CROCKFORD[time % 32] + out;
      time = Math.floor(time / 32);
    }
    const bytes = new Uint8Array(16);
    crypto.getRandomValues(bytes);
    for (let i = 0; i < bytes.length; i += 1) out += CROCKFORD[bytes[i] % 32];
    return out;
  }

  /** Send a message to the service worker without waiting for its reply. */
  function post(message) {
    try {
      const result = chrome.runtime.sendMessage(Object.assign({ source: 'corvus.offscreen' }, message));
      if (result && typeof result.catch === 'function') result.catch(() => {});
    } catch (error) {
      log('message to service worker failed', describeError(error));
    }
  }

  function reportStatus(status, error) {
    lastStatus = status;
    lastError = error || null;
    post({ type: 'status', status, lastError, url: WS_URL, at: now() });
    log('status', status, lastError || '');
  }

  function clearReconnectTimer() {
    if (reconnectTimer !== null) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
  }

  /**
   * 250 ms doubling to an 8 s cap, with +/-25% jitter, floored at 250 ms and
   * clamped at 8 s so the advertised window always holds.
   */
  function nextDelayMs() {
    const exponent = Math.min(attempt, 12);
    const base = Math.min(BACKOFF_MAX_MS, BACKOFF_MIN_MS * (2 ** exponent));
    const jitter = 0.75 + Math.random() * 0.5;
    return Math.max(BACKOFF_MIN_MS, Math.min(BACKOFF_MAX_MS, Math.round(base * jitter)));
  }

  function scheduleReconnect(reason) {
    if (reconnectTimer !== null) return;
    const delay = nextDelayMs();
    attempt += 1;
    log('reconnect scheduled in ' + delay + ' ms (' + reason + ')');
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      connect('backoff');
    }, delay);
  }

  function sendFrame(frame) {
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

  function connect(reason) {
    const state = socket ? socket.readyState : WebSocket.CLOSED;
    if (state === WebSocket.OPEN || state === WebSocket.CONNECTING) return;

    clearReconnectTimer();
    reportStatus('connecting', null);

    let next;
    try {
      next = new WebSocket(WS_URL);
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
      startTimers();
      void sendHello();
    });

    next.addEventListener('message', (event) => {
      onSocketMessage(event);
    });

    next.addEventListener('error', () => {
      // A close event always follows an error event; reconnection is handled there.
      log('socket error');
    });

    next.addEventListener('close', (event) => {
      onSocketClosed(event);
    });

    log('dialing', WS_URL, '(' + (reason || 'manual') + ')');
  }

  function onSocketMessage(event) {
    lastFrameAt = now();

    let frame;
    try {
      frame = JSON.parse(String(event.data));
    } catch (error) {
      log('discarding frame that is not JSON');
      return;
    }
    if (!frame || typeof frame !== 'object') return;

    if (frame.kind === 'ping') {
      sendFrame({ v: PROTOCOL_VERSION, id: frame.id || makeId(), kind: 'pong' });
      return;
    }
    if (frame.kind === 'pong') return;

    if (frame.kind === 'hello_ack' || frame.kind === 'command' || frame.kind === 'event') {
      post({ type: 'frame', frame });
      return;
    }

    log('ignoring inbound frame kind', frame.kind);
  }

  function onSocketClosed(event) {
    const code = event && typeof event.code === 'number' ? event.code : 0;
    const reason = event && event.reason ? String(event.reason) : '';
    const detail = code === 0 && !reason ? 'closed' : 'code=' + code + (reason ? ' reason=' + reason : '');
    socket = null;
    stopTimers();
    reportStatus('disconnected', detail);
    scheduleReconnect(detail);
  }

  async function sendHello() {
    if (helloInFlight || !socket || socket.readyState !== WebSocket.OPEN) return;
    helloInFlight = true;

    let frame = null;
    try {
      const reply = await Promise.race([
        chrome.runtime.sendMessage({ source: 'corvus.offscreen', type: 'hello_request' }),
        new Promise((resolve) => setTimeout(() => resolve(null), HELLO_TIMEOUT_MS)),
      ]);
      if (reply && reply.frame && typeof reply.frame === 'object') frame = reply.frame;
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
          capabilities: ['dom', 'scripting'],
        },
      };
      log('service worker did not answer; sending a minimal hello');
    }

    if (sendFrame(frame)) log('hello sent', frame.id);
  }

  function startTimers() {
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

  function stopTimers() {
    if (keepaliveTimer !== null) {
      clearInterval(keepaliveTimer);
      keepaliveTimer = null;
    }
    if (healthTimer !== null) {
      clearInterval(healthTimer);
      healthTimer = null;
    }
  }

  chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (!message || message.target !== 'offscreen') return false;

    switch (message.type) {
      case 'corvus.send':
        sendResponse({ ok: sendFrame(message.frame) });
        return false;
      case 'corvus.connect':
        connect(message.reason || 'service worker');
        sendResponse({ ok: true, status: lastStatus });
        return false;
      case 'corvus.status_request':
        sendResponse({ ok: true, status: lastStatus, lastError, attempt, url: WS_URL });
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

  // Keep the worker warm from the first moment the document exists, then dial.
  post({ type: 'keepalive', at: now() });
  keepaliveTimer = setInterval(() => {
    post({ type: 'keepalive', at: now() });
  }, SW_KEEPALIVE_MS);
  connect('offscreen_boot');
})();
