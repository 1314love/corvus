'use strict';

/**
 * Corvus service worker.
 *
 * Responsibilities:
 *  - own the protocol envelope handling described in docs/02-architecture.md section 4;
 *  - create a hello envelope, store the browser id assigned by hello_ack;
 *  - dispatch command envelopes to the four M1 tools and always answer them;
 *  - create and supervise the offscreen document that holds the WebSocket;
 *  - run a thirty second alarm watchdog that restarts a dead socket;
 *  - expose the debugger channel switch and the connection state to the popup.
 *
 * Plain JavaScript, MV3, no build step, no dependencies.
 */

const PROTOCOL_VERSION = 1;
const DEFAULT_TIMEOUT_MS = 15000;
const MIN_TIMEOUT_MS = 250;
const MAX_TIMEOUT_MS = 120000;
const COMMAND_TIMEOUT_PAD_MS = 1500;

const WS_URL = 'ws://127.0.0.1:7330';
const OFFSCREEN_URL = 'offscreen.html';
const OFFSCREEN_REASON = 'BLOBS';
const ALARM_WATCHDOG = 'corvus.watchdog';
const WATCHDOG_PERIOD_MINUTES = 0.5;
const RESPONSE_CACHE_LIMIT = 200;
const MAX_REFS = 5000;

const STORAGE = {
  connection: 'corvus.connection',
  debugger: 'corvus.debugger',
  profile: 'corvus.profile',
  browserId: 'corvus.browserId',
};

const RETRYABLE = {
  BAD_ARGS: false,
  NOT_FOUND: true,
  NOT_INTERACTABLE: true,
  TIMEOUT: true,
  NAVIGATION_LOST: true,
  CAPABILITY_UNAVAILABLE: false,
  BROWSER_GONE: false,
  TRANSPORT_DOWN: true,
  INTERNAL: false,
};

// Only names the protocol crate understands survive negotiation
// (crates/corvus-proto/src/handshake.rs, capability::SUPPORTED). DOM is the
// capability that makes the four M1 tools available.
const BASE_CAPABILITIES = ['dom', 'scripting'];

const CROCKFORD = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';

// The frozen M1 tool names. No aliases: the four names are the contract.
const TOOL_NAMES = ['browser_navigate', 'page_snapshot', 'browser_click', 'browser_type'];

const state = {
  status: 'disconnected',
  browserId: null,
  daemonVersion: null,
  limits: null,
  debuggerEnabled: false,
  lastError: null,
  connectedAt: null,
  profileToken: null,
};

const responseCache = new Map();
let bootPromise = null;
let creationPromise = null;

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

function log() {
  const args = Array.prototype.slice.call(arguments);
  console.debug.apply(console, ['[corvus:sw]'].concat(args));
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

function firstString() {
  for (let i = 0; i < arguments.length; i += 1) {
    const value = arguments[i];
    if (typeof value === 'string' && value.trim().length > 0) return value.trim();
  }
  return null;
}

function makeId() {
  let time = Date.now();
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

function randomToken() {
  const bytes = new Uint8Array(8);
  crypto.getRandomValues(bytes);
  let out = '';
  for (let i = 0; i < bytes.length; i += 1) out += bytes[i].toString(16).padStart(2, '0');
  return out;
}

function clampTimeout(value) {
  const numeric = Number(value);
  if (!Number.isFinite(numeric) || numeric <= 0) return DEFAULT_TIMEOUT_MS;
  return Math.min(MAX_TIMEOUT_MS, Math.max(MIN_TIMEOUT_MS, Math.round(numeric)));
}

function normalizeStatus(value) {
  if (value === 'connected' || value === 'connecting' || value === 'disconnected' || value === 'degraded') return value;
  return 'disconnected';
}

function detectBrowserName() {
  const nav = self.navigator;
  const brands = nav && nav.userAgentData && Array.isArray(nav.userAgentData.brands) ? nav.userAgentData.brands : [];
  const names = brands.map((brand) => String(brand && brand.brand ? brand.brand : '').toLowerCase());
  if (names.some((name) => name.indexOf('edge') !== -1)) return 'edge';
  if (names.some((name) => name.indexOf('brave') !== -1)) return 'brave';
  if (names.some((name) => name.indexOf('opera') !== -1)) return 'opera';
  if (names.some((name) => name.indexOf('vivaldi') !== -1)) return 'vivaldi';
  if (names.some((name) => name.indexOf('chromium') !== -1)) return 'chromium';
  if (names.some((name) => name.indexOf('chrome') !== -1)) return 'chrome';

  const ua = String((nav && nav.userAgent) || '');
  if (/Edg\//.test(ua)) return 'edge';
  if (/OPR\//.test(ua)) return 'opera';
  if (/Chrome\//.test(ua)) return 'chrome';
  return 'unknown';
}

function computeCapabilities() {
  const capabilities = BASE_CAPABILITIES.slice();
  if (state.debuggerEnabled) capabilities.push('debugger');
  return capabilities;
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

class ToolError extends Error {
  constructor(code, message, details) {
    super(message);
    this.name = 'ToolError';
    this.code = Object.prototype.hasOwnProperty.call(RETRYABLE, code) ? code : 'INTERNAL';
    this.details = details === undefined ? null : details;
  }

  toPayload(traceId) {
    const payload = {
      code: this.code,
      message: this.message,
      retryable: RETRYABLE[this.code] === true,
    };
    if (this.details !== null && this.details !== undefined) payload.details = this.details;
    if (traceId) payload.trace_id = traceId;
    return payload;
  }
}

function toErrorPayload(error, traceId) {
  if (error instanceof ToolError) return error.toPayload(traceId);
  const payload = {
    code: 'INTERNAL',
    message: describeError(error),
    retryable: false,
  };
  if (traceId) payload.trace_id = traceId;
  return payload;
}

function fromContentError(error) {
  if (!error || typeof error !== 'object') {
    return new ToolError('INTERNAL', 'content script returned a malformed error object');
  }
  const code = typeof error.code === 'string' ? error.code : 'INTERNAL';
  const message = typeof error.message === 'string' && error.message ? error.message : 'content script reported an error';
  return new ToolError(code, message, error.details || null);
}

// ---------------------------------------------------------------------------
// State, storage, popup views
// ---------------------------------------------------------------------------

async function loadState() {
  const stored = await chrome.storage.local.get([STORAGE.connection, STORAGE.debugger, STORAGE.browserId, STORAGE.profile]);

  state.debuggerEnabled = stored[STORAGE.debugger] === true;

  if (typeof stored[STORAGE.browserId] === 'string' && stored[STORAGE.browserId].trim()) {
    state.browserId = stored[STORAGE.browserId].trim();
  }

  let profile = typeof stored[STORAGE.profile] === 'string' ? stored[STORAGE.profile].trim() : '';
  if (!/^[0-9a-f]{8,32}$/.test(profile)) {
    profile = randomToken();
    try {
      await chrome.storage.local.set({ [STORAGE.profile]: profile });
    } catch (error) {
      log('profile token could not be stored', describeError(error));
    }
  }
  state.profileToken = profile;

  const connection = stored[STORAGE.connection];
  if (connection && typeof connection === 'object' && typeof connection.status === 'string') {
    state.status = normalizeStatus(connection.status);
  }
  log('state loaded', state.status, state.profileToken);
}

const stateReady = loadState().catch((error) => {
  log('state load failed', describeError(error));
});

async function persistConnection() {
  const snapshot = {
    status: state.status,
    browserId: state.browserId,
    daemonVersion: state.daemonVersion,
    debuggerEnabled: state.debuggerEnabled,
    lastError: state.lastError,
    updatedAt: Date.now(),
  };
  try {
    await chrome.storage.local.set({ [STORAGE.connection]: snapshot });
  } catch (error) {
    log('connection state could not be stored', describeError(error));
  }
}

function updateStatus(status, lastError) {
  const nextStatus = normalizeStatus(status);
  const nextError = lastError ? String(lastError) : null;
  const changed = state.status !== nextStatus || state.lastError !== nextError;
  state.status = nextStatus;
  state.lastError = nextError;
  if (nextStatus === 'connected') {
    if (!state.connectedAt) state.connectedAt = Date.now();
  } else {
    state.connectedAt = null;
  }
  if (changed) void persistConnection();
}

function buildUiState() {
  return {
    status: state.status,
    connected: state.status === 'connected',
    browserId: state.browserId,
    daemonVersion: state.daemonVersion,
    debuggerEnabled: state.debuggerEnabled,
    lastError: state.lastError,
    wsUrl: WS_URL,
  };
}

// ---------------------------------------------------------------------------
// Offscreen document lifecycle
// ---------------------------------------------------------------------------

async function offscreenExists() {
  try {
    if (chrome.offscreen && typeof chrome.offscreen.hasDocument === 'function') {
      return await chrome.offscreen.hasDocument();
    }
  } catch (error) {
    log('hasDocument failed', describeError(error));
  }
  try {
    if (chrome.runtime.getContexts && typeof chrome.runtime.getContexts === 'function') {
      const contexts = await chrome.runtime.getContexts({ contextTypes: ['OFFSCREEN_DOCUMENT'] });
      return Array.isArray(contexts) && contexts.length > 0;
    }
  } catch (error) {
    log('getContexts failed', describeError(error));
  }
  return false;
}

async function ensureOffscreen() {
  if (await offscreenExists()) return true;
  if (creationPromise) return creationPromise;

  creationPromise = (async () => {
    try {
      await chrome.offscreen.createDocument({
        url: OFFSCREEN_URL,
        reasons: [OFFSCREEN_REASON],
        justification: 'Owns the loopback WebSocket and a heartbeat so the service worker is not suspended between commands.',
      });
      log('offscreen document created');
      return true;
    } catch (error) {
      const message = describeError(error);
      if (/single offscreen|already exists/i.test(message)) return true;
      log('offscreen document creation failed', message);
      return false;
    } finally {
      creationPromise = null;
    }
  })();

  return creationPromise;
}

async function sendToOffscreen(message) {
  const payload = Object.assign({ target: 'offscreen' }, message);
  try {
    return await chrome.runtime.sendMessage(payload);
  } catch (error) {
    log('message to offscreen failed, retrying once', describeError(error));
    await ensureOffscreen();
    return chrome.runtime.sendMessage(payload);
  }
}

// ---------------------------------------------------------------------------
// Envelopes: hello, events, responses
// ---------------------------------------------------------------------------

async function buildHello() {
  await stateReady;
  const manifest = chrome.runtime.getManifest();
  const browserName = detectBrowserName();
  const profileToken = state.profileToken || 'default';
  return {
    v: PROTOCOL_VERSION,
    id: makeId(),
    kind: 'hello',
    browser: browserName + ':' + profileToken,
    args: {
      profile: profileToken,
      version: manifest.version,
      capabilities: computeCapabilities(),
    },
  };
}

function buildEvent(name, payload) {
  return {
    v: PROTOCOL_VERSION,
    id: makeId(),
    kind: 'event',
    browser: state.browserId || undefined,
    tool: name,
    args: payload || {},
  };
}

async function sendCapabilitiesEvent(reason) {
  const event = buildEvent('capabilities_changed', {
    capabilities: computeCapabilities(),
    reason: reason || 'local',
  });
  try {
    const reply = await sendToOffscreen({ type: 'corvus.send', frame: event });
    if (!reply || reply.ok !== true) log('capabilities event dropped: socket is closed');
  } catch (error) {
    log('capabilities event could not be sent', describeError(error));
  }
}

function buildResponse(frame, ok, result, error) {
  const traceId = frame && frame.meta && frame.meta.trace_id ? String(frame.meta.trace_id) : null;
  const response = {
    v: PROTOCOL_VERSION,
    id: String((frame && frame.id) || makeId()),
    kind: 'response',
    browser: state.browserId || (frame && frame.browser) || undefined,
    session: frame && frame.session ? frame.session : undefined,
    tool: frame && frame.tool ? frame.tool : undefined,
    args: ok ? (result === undefined ? null : result) : null,
    meta: traceId ? { trace_id: traceId } : undefined,
  };
  if (!ok) {
    response.error = error || { code: 'INTERNAL', message: 'unknown failure', retryable: false };
  }
  return response;
}

async function sendFrame(frame) {
  if (!frame || typeof frame !== 'object') return false;
  try {
    const reply = await sendToOffscreen({ type: 'corvus.send', frame });
    if (reply && reply.ok === false) {
      log('response not delivered: socket is closed');
      return false;
    }
    return true;
  } catch (error) {
    log('response could not be delivered', describeError(error));
    return false;
  }
}

function handleHelloAck(frame) {
  const args = frame && frame.args && typeof frame.args === 'object' ? frame.args : {};
  const assigned = firstString(frame.browser, args.browser, args.browser_id, args.browserId, args.assigned_browser, args.assignedBrowser, args.id);
  if (assigned) {
    state.browserId = assigned;
    chrome.storage.local.set({ [STORAGE.browserId]: assigned }).catch(() => {});
  }
  state.daemonVersion = firstString(args.daemon_version, args.daemonVersion, args.version, frame.version) || null;
  state.limits = args.limits !== undefined ? args.limits : null;
  updateStatus('connected', null);
  void persistConnection();
  log('hello_ack: browser=' + (state.browserId || 'unassigned') + ' daemon=' + (state.daemonVersion || 'unknown'));
}

function handleDaemonEvent(frame) {
  const args = frame && frame.args && typeof frame.args === 'object' ? frame.args : {};
  const name = firstString(args.event, args.name, args.type, frame && frame.tool);
  if (!name) return;

  if (name === 'capabilities_request' || name === 'hello_request') {
    void sendCapabilitiesEvent('event_request');
    return;
  }
  if (name === 'resume') {
    void sendToOffscreen({ type: 'corvus.connect', reason: 'daemon_resume' }).catch(() => {});
    return;
  }
  log('ignoring daemon event', name);
}

// ---------------------------------------------------------------------------
// Command routing
// ---------------------------------------------------------------------------

const HANDLERS = {
  browser_navigate: (args, frame) => navigateTool(args, frame),
  page_snapshot: (args, frame) => runInTab('page_snapshot', args, frame),
  browser_click: (args, frame) => runInTab('browser_click', args, frame),
  browser_type: (args, frame) => runInTab('browser_type', args, frame),
};

function idempotencyKeyOf(frame) {
  const key = frame && frame.meta && frame.meta.idempotency_key;
  return typeof key === 'string' && key.trim() ? key.trim() : null;
}

function rememberResponse(key, response) {
  if (!key) return;
  responseCache.set(key, response);
  while (responseCache.size > RESPONSE_CACHE_LIMIT) {
    const oldest = responseCache.keys().next();
    if (oldest.done) break;
    responseCache.delete(oldest.value);
  }
}

function withTimeout(promise, timeoutMs) {
  let timer = null;
  const timeout = new Promise((resolve, reject) => {
    timer = setTimeout(() => {
      reject(new ToolError('TIMEOUT', 'command exceeded ' + timeoutMs + ' ms'));
    }, timeoutMs);
  });
  return Promise.race([promise, timeout]).finally(() => {
    if (timer !== null) clearTimeout(timer);
  });
}

async function handleCommand(frame) {
  const startedAt = Date.now();
  const traceId = frame && frame.meta && frame.meta.trace_id ? String(frame.meta.trace_id) : null;
  const key = idempotencyKeyOf(frame);

  if (key) {
    const cached = responseCache.get(key);
    if (cached) {
      const replay = Object.assign({}, cached, { id: String((frame && frame.id) || cached.id) });
      log('replaying cached response for idempotency key', key);
      await sendFrame(replay);
      return;
    }
  }

  let response;
  try {
    const rawTool = firstString(frame && frame.tool);
    if (!rawTool) throw new ToolError('BAD_ARGS', 'command envelope is missing tool');
    const tool = rawTool.toLowerCase();
    if (TOOL_NAMES.indexOf(tool) === -1) throw new ToolError('BAD_ARGS', 'unknown tool: ' + rawTool, { tool: rawTool, known: TOOL_NAMES });

    const handler = HANDLERS[tool];
    if (typeof handler !== 'function') throw new ToolError('INTERNAL', 'no handler registered for ' + tool);

    const args = frame.args && typeof frame.args === 'object' ? frame.args : {};
    const timeoutMs = clampTimeout(frame.meta && frame.meta.timeout_ms);
    const result = await withTimeout(handler(args, frame), timeoutMs + COMMAND_TIMEOUT_PAD_MS);
    response = buildResponse(frame, true, result, null);
  } catch (error) {
    response = buildResponse(frame, false, null, toErrorPayload(error, traceId));
  }

  rememberResponse(key, response);
  log('command', (frame && frame.tool) || '?', 'answered in', Date.now() - startedAt, 'ms');
  await sendFrame(response);
}

async function handleInboundFrame(frame) {
  if (!frame || typeof frame !== 'object') return;
  switch (frame.kind) {
    case 'hello_ack':
      handleHelloAck(frame);
      return;
    case 'command':
      await handleCommand(frame);
      return;
    case 'event':
      handleDaemonEvent(frame);
      return;
    case 'ping':
      await sendFrame({ v: PROTOCOL_VERSION, id: String(frame.id || makeId()), kind: 'pong', browser: state.browserId || undefined });
      return;
    case 'pong':
    case 'response':
      return;
    default:
      log('ignoring inbound frame kind', frame.kind);
  }
}

// ---------------------------------------------------------------------------
// Tabs, content scripts
// ---------------------------------------------------------------------------

const RESTRICTED_PROTOCOLS = ['chrome:', 'chrome-extension:', 'chrome-untrusted:', 'devtools:', 'edge:', 'brave:', 'opera:', 'vivaldi:', 'view-source:', 'about:'];

function isRestrictedUrl(url) {
  if (!url) return false;
  const lowered = String(url).trim().toLowerCase();
  if (lowered === '' || lowered === 'about:blank') return false;
  return RESTRICTED_PROTOCOLS.some((protocol) => lowered.startsWith(protocol));
}

async function resolveTab(args) {
  const raw = args ? (args.tab_id !== undefined ? args.tab_id : args.tabId) : undefined;
  if (raw !== undefined && raw !== null && raw !== '') {
    const tabId = Number(raw);
    if (!Number.isInteger(tabId) || tabId < 0) {
      throw new ToolError('BAD_ARGS', 'args.tab_id must be a non-negative integer', { tab_id: raw });
    }
    try {
      return await chrome.tabs.get(tabId);
    } catch (error) {
      throw new ToolError('NOT_FOUND', 'tab ' + tabId + ' does not exist', { tab_id: tabId });
    }
  }

  const queries = [
    { active: true, lastFocusedWindow: true },
    { active: true, currentWindow: true },
  ];
  for (let i = 0; i < queries.length; i += 1) {
    try {
      const tabs = await chrome.tabs.query(queries[i]);
      if (Array.isArray(tabs) && tabs.length > 0) return tabs[0];
    } catch (error) {
      log('tabs.query failed', describeError(error));
    }
  }
  return null;
}

function isNoReceiverError(error) {
  return /Receiving end does not exist|Could not establish connection/i.test(describeError(error));
}

function isPortClosedError(error) {
  return /message port closed|Could not establish connection/i.test(describeError(error));
}

function classifyInjectionFailure(error) {
  const message = describeError(error);
  if (/Cannot access|extensions gallery cannot be scripted|cannot be scripted/i.test(message)) {
    return new ToolError('CAPABILITY_UNAVAILABLE', 'this page cannot be scripted: ' + message, { reason: 'restricted_scheme' });
  }
  if (/was closed|has been closed|Frame with ID .* was removed/i.test(message)) {
    return new ToolError('BROWSER_GONE', 'the tab went away before the command could run', { reason: 'tab_closed' });
  }
  if (/No tab with id|Invalid tab ID/i.test(message)) {
    return new ToolError('NOT_FOUND', 'the target tab does not exist', { reason: 'tab_missing' });
  }
  return new ToolError('INTERNAL', 'content script injection failed: ' + message);
};

async function ensureContentScript(tabId, force) {
  if (!force) {
    try {
      const pong = await chrome.tabs.sendMessage(tabId, { type: 'corvus.ping' });
      if (pong && pong.ok === true) return;
    } catch (error) {
      // No receiver yet: inject below.
    }
  }
  try {
    await chrome.scripting.executeScript({ target: { tabId, allFrames: false }, files: ['content.js'] });
  } catch (error) {
    throw classifyInjectionFailure(error);
  }
}

async function sendContentCommand(tabId, message) {
  try {
    return await chrome.tabs.sendMessage(tabId, message);
  } catch (error) {
    if (!isNoReceiverError(error)) throw error;
    await ensureContentScript(tabId, true);
    return chrome.tabs.sendMessage(tabId, message);
  }
}

function classifyContentFailure(error, tab) {
  const message = describeError(error);
  if (/message port closed|The page was reloaded|navigated/i.test(message)) {
    return new ToolError('NAVIGATION_LOST', 'the page navigated before the command answered', { url: tab && tab.url ? tab.url : null });
  }
  if (/No tab with id|Invalid tab ID/i.test(message)) {
    return new ToolError('NOT_FOUND', 'the target tab does not exist', { reason: 'tab_missing' });
  }
  if (/was closed|has been closed|Frame with ID .* was removed/i.test(message)) {
    return new ToolError('BROWSER_GONE', 'the tab was closed while the command was running', { reason: 'tab_closed' });
  }
  if (/Cannot access/i.test(message)) {
    return new ToolError('CAPABILITY_UNAVAILABLE', 'this page cannot be scripted', { reason: 'restricted_scheme' });
  }
  return new ToolError('INTERNAL', 'content script reply failed: ' + message);
}

async function runInTab(tool, args, frame) {
  const tab = await resolveTab(args);
  if (!tab || typeof tab.id !== 'number') {
    throw new ToolError('NOT_FOUND', 'no active tab is available');
  }
  if (isRestrictedUrl(tab.url)) {
    throw new ToolError('CAPABILITY_UNAVAILABLE', 'this page cannot be scripted: ' + String(tab.url), { url: tab.url, reason: 'restricted_scheme' });
  }

  await ensureContentScript(tab.id, false);

  const message = {
    type: 'corvus.command',
    id: String((frame && frame.id) || makeId()),
    tool,
    args: args || {},
    meta: frame && frame.meta ? frame.meta : {},
  };

  let reply;
  try {
    reply = await sendContentCommand(tab.id, message);
  } catch (error) {
    throw classifyContentFailure(error, tab);
  }

  if (!reply || typeof reply !== 'object') {
    throw new ToolError('INTERNAL', 'content script returned an empty reply');
  }
  if (reply.ok === false) throw fromContentError(reply.error);
  return reply.result;
}

// ---------------------------------------------------------------------------
// Navigation: issue, wait for the new document, read the final URL
// ---------------------------------------------------------------------------

const navigationEpochs = new Map();

function beginNavigationEpoch(tabId) {
  const next = (navigationEpochs.get(tabId) || 0) + 1;
  navigationEpochs.set(tabId, next);
  return next;
}

function isErrorPageUrl(url) {
  return typeof url === 'string' && /^(chrome|edge)-error:\/\//i.test(url.trim());
}

function isNavigableUrl(url) {
  return typeof url === 'string' && /^(https?|file):/i.test(url.trim());
}

function sameDocumentExceptHash(left, right) {
  try {
    const a = new URL(left);
    const b = new URL(right);
    return a.origin === b.origin && a.pathname === b.pathname && a.search === b.search;
  } catch (error) {
    return false;
  }
}

async function updateTabUrl(tabId, url) {
  try {
    await chrome.tabs.update(tabId, { url: url });
  } catch (error) {
    const message = describeError(error);
    if (/No tab with id|Invalid tab ID/i.test(message)) {
      throw new ToolError('BROWSER_GONE', 'the tab was closed before navigation could start', { reason: 'tab_closed' });
    }
    throw new ToolError('INTERNAL', 'navigation could not be started: ' + message);
  }
}

function waitForDocumentLoad(tabId, beforeUrl, targetUrl, epoch, timeoutMs) {
  return new Promise((resolve) => {
    let settled = false;
    let sawLoading = false;
    let timer = null;

    function cleanup() {
      try {
        chrome.tabs.onUpdated.removeListener(onUpdated);
      } catch (error) {
        // Ignore.
      }
      if (timer !== null) {
        clearTimeout(timer);
        timer = null;
      }
    }

    function finish(loaded, url) {
      if (settled) return;
      settled = true;
      cleanup();
      resolve({ loaded: loaded === true, url: url || null });
    }

    function onUpdated(updatedTabId, changeInfo, tab) {
      if (settled || updatedTabId !== tabId) return;
      // A newer navigate command owns this tab; never resolve its wait.
      if ((navigationEpochs.get(tabId) || 0) !== epoch) {
        finish(false, tab && tab.url ? tab.url : null);
        return;
      }
      if (!changeInfo) return;
      if (changeInfo.status === 'loading') {
        sawLoading = true;
        return;
      }
      if (changeInfo.status !== 'complete') return;
      const url = (tab && tab.url) || changeInfo.url || null;
      // Ignore a late complete event for the document that was already loaded,
      // except for an explicit reload of the same URL where no loading event
      // was observed before the listener attached.
      const sameUrlReload = targetUrl === beforeUrl;
      if (!sawLoading && !sameUrlReload && beforeUrl && url === beforeUrl) return;
      finish(true, url);
    }

    chrome.tabs.onUpdated.addListener(onUpdated);

    timer = setTimeout(async () => {
      if (settled) return;
      let url = null;
      try {
        const tab = await chrome.tabs.get(tabId);
        url = tab && tab.url ? tab.url : null;
      } catch (error) {
        url = null;
      }
      finish(false, url);
    }, Math.max(1, timeoutMs));

    // The load may already be complete before the listener was attached.
    chrome.tabs.get(tabId).then((tab) => {
      if (settled || !tab) return;
      if ((navigationEpochs.get(tabId) || 0) !== epoch) {
        finish(false, tab.url || null);
        return;
      }
      const url = tab.url || null;
      if (tab.status === 'complete' && url && (!beforeUrl || url !== beforeUrl)) finish(true, url);
    }).catch(() => {
      // The timeout path reports the final state.
    });
  });
}

async function navigateTool(args, frame) {
  const tab = await resolveTab(args);
  if (!tab || typeof tab.id !== 'number') {
    throw new ToolError('NOT_FOUND', 'no active tab is available');
  }

  const raw = args && typeof args.url === 'string' ? args.url : (args && typeof args.href === 'string' ? args.href : '');
  if (!raw.trim()) throw new ToolError('BAD_ARGS', 'args.url must be a non-empty string');

  const beforeUrl = tab.url || null;
  const baseUrl = beforeUrl && !isErrorPageUrl(beforeUrl) ? beforeUrl : 'about:blank';
  let resolved = null;
  try {
    resolved = new URL(raw.trim(), baseUrl);
  } catch (error) {
    throw new ToolError('BAD_ARGS', 'args.url is not a valid URL', { url: raw.slice(0, 200) });
  }
  if (!/^(https?|file):$/i.test(resolved.protocol)) {
    throw new ToolError('BAD_ARGS', 'only http, https and file URLs can be navigated to', { url: resolved.href.slice(0, 200) });
  }
  const target = resolved.href;

  const timeoutMs = clampTimeout(frame && frame.meta ? frame.meta.timeout_ms : undefined);
  // Keep at least 2 s of the tool budget for the reply and stay below 15 s.
  const waitBudget = Math.max(0, Math.min(timeoutMs - 2000, 15000));
  const epoch = beginNavigationEpoch(tab.id);

  let started = null;
  if (isErrorPageUrl(beforeUrl)) {
    // An error page cannot host a content script; navigate it directly.
    await updateTabUrl(tab.id, target);
    started = { navigated: true, previous: beforeUrl, started_at: Date.now() };
  } else {
    try {
      started = await runInTab('browser_navigate', args, frame);
    } catch (error) {
      if (!/showing error page/i.test(describeError(error))) throw error;
      await updateTabUrl(tab.id, target);
      started = { navigated: true, previous: beforeUrl, started_at: Date.now() };
    }
  }

  if (!started || typeof started !== 'object') {
    throw new ToolError('INTERNAL', 'navigation did not return a result');
  }

  const hashOnly = beforeUrl !== null && target !== beforeUrl && sameDocumentExceptHash(beforeUrl, target);
  let loaded = hashOnly === true;
  let finalUrl = target;

  if (!hashOnly && waitBudget > 0) {
    const outcome = await waitForDocumentLoad(tab.id, beforeUrl, target, epoch, waitBudget);
    loaded = outcome.loaded;
    finalUrl = outcome.url || target;
  }

  let current = null;
  try {
    current = await chrome.tabs.get(tab.id);
  } catch (error) {
    throw new ToolError('BROWSER_GONE', 'the tab was closed while navigation was in progress', { reason: 'tab_closed' });
  }
  if (current && current.url && !(hashOnly && current.url === beforeUrl)) finalUrl = current.url;
  if (!hashOnly && waitBudget <= 0) {
    // No waiting budget left: only claim loaded when the URL has clearly moved.
    loaded = current.status === 'complete' && (!beforeUrl || current.url !== beforeUrl);
  }

  // Inject once after the load wait so the next snapshot sees the new document.
  if (!hashOnly) {
    try {
      if (isNavigableUrl(finalUrl) && !isErrorPageUrl(finalUrl)) {
        await ensureContentScript(tab.id, false);
      }
    } catch (error) {
      log('post-navigation injection retry failed', describeError(error));
    }
  }

  return {
    navigated: true,
    loaded: loaded === true,
    url: finalUrl,
    previous: started.previous !== undefined ? started.previous : beforeUrl,
    same_document: hashOnly === true,
    started_at: started.started_at || Date.now(),
  };
}

// ---------------------------------------------------------------------------
// Debugger channel
// ---------------------------------------------------------------------------

async function attachDebuggerToActiveTab() {
  const tab = await resolveTab({});
  if (!tab || typeof tab.id !== 'number') {
    throw new ToolError('NOT_FOUND', 'no active tab to attach the debugger to');
  }
  if (isRestrictedUrl(tab.url)) {
    throw new ToolError('CAPABILITY_UNAVAILABLE', 'this page cannot be debugged: ' + String(tab.url), { url: tab.url });
  }

  let targets = [];
  try {
    targets = await chrome.debugger.getTargets();
  } catch (error) {
    log('debugger.getTargets failed', describeError(error));
  }
  const existing = (targets || []).find((target) => target && target.tabId === tab.id && target.attached === true);
  if (existing) return;

  await chrome.debugger.attach({ tabId: tab.id }, '1.3');
}

async function detachDebuggerFromAllTabs() {
  let targets = [];
  try {
    targets = await chrome.debugger.getTargets();
  } catch (error) {
    log('debugger.getTargets failed', describeError(error));
    return;
  }
  const attached = (targets || []).filter((target) => target && target.attached === true && typeof target.tabId === 'number' && target.tabId >= 0);
  for (let i = 0; i < attached.length; i += 1) {
    try {
      await chrome.debugger.detach({ tabId: attached[i].tabId });
    } catch (error) {
      log('debugger.detach failed', describeError(error));
    }
  }
}

async function setDebuggerEnabled(enabled, source) {
  const next = enabled === true;
  const previous = state.debuggerEnabled;
  state.debuggerEnabled = next;

  try {
    await chrome.storage.local.set({ [STORAGE.debugger]: next });
  } catch (error) {
    log('debugger state could not be stored', describeError(error));
  }

  if (next) {
    try {
      await attachDebuggerToActiveTab();
      state.lastError = null;
    } catch (error) {
      state.lastError = describeError(error);
      log('debugger attach failed', state.lastError);
    }
  } else {
    await detachDebuggerFromAllTabs();
  }

  if (previous !== next) {
    await persistConnection();
    if (state.status === 'connected') void sendCapabilitiesEvent(source || 'popup');
  }

  return state.debuggerEnabled;
}

// ---------------------------------------------------------------------------
// Watchdog and boot
// ---------------------------------------------------------------------------

async function ensureWatchdog() {
  try {
    await chrome.alarms.create(ALARM_WATCHDOG, { periodInMinutes: WATCHDOG_PERIOD_MINUTES });
  } catch (error) {
    log('watchdog alarm could not be created', describeError(error));
  }
}

async function watchdogTick() {
  try {
    const exists = await ensureOffscreen();
    if (!exists) {
      updateStatus('disconnected', 'offscreen document unavailable');
      return;
    }
    const status = await sendToOffscreen({ type: 'corvus.status_request' });
    const reported = status && typeof status.status === 'string' ? normalizeStatus(status.status) : 'disconnected';
    updateStatus(reported, status && status.lastError ? status.lastError : null);
    if (reported !== 'connected') {
      await sendToOffscreen({ type: 'corvus.connect', reason: 'watchdog' });
    }
  } catch (error) {
    const message = describeError(error);
    log('watchdog tick failed', message);
    updateStatus('disconnected', message);
    void ensureOffscreen();
  }
}

async function doBoot(reason) {
  log('boot', reason || '');
  await stateReady;
  await ensureWatchdog();

  const exists = await ensureOffscreen();
  if (!exists) {
    updateStatus('disconnected', 'offscreen document unavailable');
    return;
  }

  try {
    await sendToOffscreen({ type: 'corvus.connect', reason: reason || 'boot' });
    const status = await sendToOffscreen({ type: 'corvus.status_request' });
    if (status && typeof status.status === 'string') {
      updateStatus(normalizeStatus(status.status), status.lastError ? status.lastError : null);
    }
  } catch (error) {
    updateStatus('disconnected', describeError(error));
  }
}

function boot(reason) {
  if (bootPromise) return bootPromise;
  bootPromise = doBoot(reason).catch((error) => {
    log('boot failed', describeError(error));
  }).finally(() => {
    bootPromise = null;
  });
  return bootPromise;
}

// ---------------------------------------------------------------------------
// Message plumbing
// ---------------------------------------------------------------------------

function handleOffscreenMessage(message) {
  if (!message || typeof message !== 'object') return;
  switch (message.type) {
    case 'status':
      updateStatus(message.status, message.lastError ? message.lastError : null);
      return;
    case 'keepalive':
      return;
    case 'frame':
      handleInboundFrame(message.frame).catch((error) => {
        log('inbound frame handling failed', describeError(error));
      });
      return;
    default:
      log('unknown offscreen message', message.type);
  }
}

async function handleUiMessage(message) {
  switch (message.type) {
    case 'corvus.get_state':
      return { ok: true, state: buildUiState() };
    case 'corvus.set_debugger': {
      const enabled = await setDebuggerEnabled(message.enabled === true, 'popup');
      return { ok: true, enabled, state: buildUiState() };
    }
    case 'corvus.reconnect':
      await sendToOffscreen({ type: 'corvus.connect', reason: 'ui' });
      return { ok: true, state: buildUiState() };
    default:
      return { ok: false, error: 'unknown ui message' };
  }
}

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (!message || typeof message !== 'object') return false;

  if (message.source === 'corvus.offscreen') {
    if (message.type === 'hello_request') {
      buildHello()
        .then((frame) => sendResponse({ ok: true, frame }))
        .catch((error) => sendResponse({ ok: false, error: describeError(error) }));
      return true;
    }
    if (message.type === 'frame') {
      handleInboundFrame(message.frame)
        .catch((error) => log('inbound frame handling failed', describeError(error)))
        .finally(() => sendResponse({ ok: true }));
      return true;
    }
    handleOffscreenMessage(message);
    sendResponse({ ok: true });
    return false;
  }

  if (typeof message.type === 'string' && message.type.indexOf('corvus.') === 0) {
    handleUiMessage(message)
      .then((reply) => sendResponse(reply))
      .catch((error) => sendResponse({ ok: false, error: describeError(error) }));
    return true;
  }

  return false;
});

chrome.alarms.onAlarm.addListener((alarm) => {
  if (!alarm || alarm.name !== ALARM_WATCHDOG) return;
  void watchdogTick();
});

chrome.debugger.onDetach.addListener((source, reason) => {
  if (reason !== 'canceled_by_user') return;
  if (!state.debuggerEnabled) return;
  state.debuggerEnabled = false;
  chrome.storage.local.set({ [STORAGE.debugger]: false }).catch(() => {});
  void persistConnection();
  if (state.status === 'connected') void sendCapabilitiesEvent('debugger_detached');
});

chrome.storage.onChanged.addListener((changes, areaName) => {
  if (areaName !== 'local' || !changes[STORAGE.debugger]) return;
  const value = changes[STORAGE.debugger].newValue === true;
  if (value !== state.debuggerEnabled) void setDebuggerEnabled(value, 'storage');
});

chrome.runtime.onInstalled.addListener(() => {
  void boot('installed');
});

chrome.runtime.onStartup.addListener(() => {
  void boot('startup');
});

self.addEventListener('online', () => {
  void sendToOffscreen({ type: 'corvus.connect', reason: 'network_online' }).catch(() => {});
});

void boot('service_worker_start');
