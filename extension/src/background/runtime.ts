import {
  DEFAULT_DAEMON_URL,
  DAEMON_URL_STORAGE_KEY,
  normalizeDaemonUrl,
  readDaemonUrl,
  resetDaemonUrl,
  writeDaemonUrl,
} from '../shared/storage/daemon-url';
import {
  DEFAULT_CDP_ENDPOINT,
  CDP_ENDPOINT_STORAGE_KEY,
  normalizeCdpEndpoint,
  probeCdpEndpoint,
  readCdpEndpoint,
  resetCdpEndpoint,
  writeCdpEndpoint,
} from '../shared/cdp-endpoint';
import { ToolError, toToolErrorPayload as toErrorPayload, policyDeniedReasonFromDetails } from '../shared/errors';
import type { CdpProbeResult, CdpSource } from '../shared/cdp-endpoint';
import type { CdpUiState, UiState } from '../shared/contracts/ui';
import type { WireCdpDescriptor } from '../shared/contracts/protocol';
import { captureScreenshot as captureScreenshotImage } from './screenshot';
import type { ScreenshotElementRect } from './screenshot';
import { createExpandedHandlers, EXPANDED_TOOL_NAMES } from './expanded-tools';

/**
 * @fileoverview Corvus service worker.
 *
 * Owns the protocol envelope flow, the offscreen socket host and the watchdog,
 * and answers every command exactly once. Plain JavaScript, MV3, no dependencies.
 */

const PROTOCOL_VERSION = 1;
const DEFAULT_TIMEOUT_MS = 60000;
const MIN_TIMEOUT_MS = 250;
const MAX_TIMEOUT_MS = 125000;
const COMMAND_TIMEOUT_PAD_MS = 1500;

const DAEMON_URL_HELPER = Object.freeze({
  DEFAULT_DAEMON_URL,
  STORAGE_KEY: DAEMON_URL_STORAGE_KEY,
  normalize: normalizeDaemonUrl,
  readDaemonUrl,
  writeDaemonUrl,
  resetDaemonUrl,
});
const CDP_ENDPOINT_HELPER = Object.freeze({
  DEFAULT_CDP_ENDPOINT,
  STORAGE_KEY: CDP_ENDPOINT_STORAGE_KEY,
  normalize: normalizeCdpEndpoint,
  probe: probeCdpEndpoint,
  readCdpEndpoint,
  writeCdpEndpoint,
  resetCdpEndpoint,
});
const OFFSCREEN_URL = 'offscreen.html';
const OFFSCREEN_REASON = 'BLOBS';
const ALARM_WATCHDOG = 'corvus.watchdog';
const WATCHDOG_PERIOD_MINUTES = 0.5;
const RESPONSE_CACHE_LIMIT = 200;
const MAX_COMMAND_CONTROLLERS = 200;
const MAX_REFS = 5000;
const CDP_IDLE_TIMEOUT_MS = 30000;
const CDP_ATTACH_TIMEOUT_MS = 5000;
const CDP_PROTOCOL_VERSION = '1.3';

// Network capture, interception, download and upload limits.
const NET_CAPTURE_CAPACITY = 2000;
const MAX_INTERCEPT_RULES = 50;
const MAX_INTERCEPT_RULE_ID_LENGTH = 64;
const MAX_INTERCEPT_BODY_BYTES = 1048576;
const MAX_UPLOAD_FILES = 10;
const DOWNLOAD_POLL_MS = 100;
const DOWNLOAD_RECENT_WINDOW_MS = 5000;

const STORAGE = {
  connection: 'corvus.connection',
  profile: 'corvus.profile',
  browserId: 'corvus.browserId',
  daemonUrl: DAEMON_URL_HELPER.STORAGE_KEY,
  cdpEndpoint: CDP_ENDPOINT_HELPER.STORAGE_KEY,
};

// Only names the protocol crate understands survive negotiation
// (crates/corvus-proto/src/handshake.rs, capability::SUPPORTED). The debugger
// capability is always advertised: the channel is managed automatically and
// attaches only when a command actually needs it.
const BASE_CAPABILITIES = ['dom', 'scripting', 'debugger'];

const CROCKFORD = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';

// Tool names handled by the extension. No aliases. browser_list is answered by
// the daemon from its live registry.
const TOOL_NAMES = [
  'browser_navigate',
  'page_snapshot',
  'browser_click',
  'browser_type',
  'page_text',
  'page_html',
  'page_find',
  'browser_hover',
  'browser_press',
  'browser_scroll',
  'browser_select',
  'browser_eval',
  'browser_wait',
  'browser_history',
  'tab_list',
  'tab_new',
  'tab_close',
  'tab_switch',
  'net_capture',
  'net_log',
  'net_clear',
  'net_intercept',
  'download_list',
  'download_wait',
  'upload_file',
  'browser_screenshot',
  ...EXPANDED_TOOL_NAMES,
];

/** Connection lifecycle status reported to the daemon and UI. */
type ConnectionStatus = 'connected' | 'connecting' | 'disconnected' | 'degraded';

/**
 * Legacy runtime state. Protocol payloads remain dynamically shaped during the
 * strangler migration; contract-bearing fields use the shared CDP type.
 */
interface RuntimeState {
  status: ConnectionStatus;
  wsUrl: string;
  browserId: string | null;
  daemonVersion: string | null;
  limits: unknown;
  lastError: string | null;
  connectedAt: number | null;
  profileToken: string | null;
  cdp: CdpProbeResult | null;
  cdpEndpoint: string;
  cdpDetectedAt: number | null;
  lastPolicyDeniedReason: string | null;
  lastPolicyDeniedAt: number | null;
}

const state: RuntimeState = {
  status: 'disconnected',
  wsUrl: DAEMON_URL_HELPER.DEFAULT_DAEMON_URL,
  browserId: null,
  daemonVersion: null,
  limits: null,
  lastError: null,
  connectedAt: null,
  profileToken: null,
  cdp: null,
  cdpEndpoint: CDP_ENDPOINT_HELPER.DEFAULT_CDP_ENDPOINT,
  cdpDetectedAt: null,
  lastPolicyDeniedReason: null,
  lastPolicyDeniedAt: null,
};

/** Opaque daemon responses and per-tab runtime records retained by the legacy runtime. */
const responseCache = new Map<string, any>();
const commandControllers = new Map<string, AbortController>();
const debuggerSessions = new Map<number, any>();
const debuggerAttachPromises = new Map<number, Promise<any>>();
const networkCaptures = new Map<number, any>();
const tabInterceptRules = new Map<number, any>();
let browserInterceptRules: any[] = [];
let networkSeq = 0;
let bootPromise: Promise<void> | null = null;
let creationPromise: Promise<boolean> | null = null;
let cdpDetectionPromise: Promise<CdpProbeResult | null> | null = null;

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

function log(...args: any[]): void {
  console.debug.apply(console, ['[corvus:sw]'].concat(args));
}

function describeError(error: any): string {
  if (!error) return 'unknown error';
  if (typeof error === 'string') return error;
  if (error.message) return String(error.message);
  try {
    return JSON.stringify(error);
  } catch (_) {
    return String(error);
  }
}

function firstString(...values: any[]): string | null {
  for (let i = 0; i < values.length; i += 1) {
    const value = values[i];
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

function clampTimeout(value: any) {
  const numeric = Number(value);
  if (!Number.isFinite(numeric) || numeric <= 0) return DEFAULT_TIMEOUT_MS;
  return Math.min(MAX_TIMEOUT_MS, Math.max(MIN_TIMEOUT_MS, Math.round(numeric)));
}

function normalizeStatus(value: any) {
  if (value === 'connected' || value === 'connecting' || value === 'disconnected' || value === 'degraded') return value;
  return 'disconnected';
}

function detectBrowserName() {
  // Kept as-is so the hello browser identity is stable across releases. The
  // CDP descriptor carries the separate browser_family field.
  const nav: any = self.navigator;
  const brands = nav && nav.userAgentData && Array.isArray(nav.userAgentData.brands) ? nav.userAgentData.brands : [];
  const names = brands.map((brand: any) => String(brand && brand.brand ? brand.brand : '').toLowerCase());
  if (names.some((name: any) => name.indexOf('edge') !== -1)) return 'edge';
  if (names.some((name: any) => name.indexOf('brave') !== -1)) return 'brave';
  if (names.some((name: any) => name.indexOf('opera') !== -1)) return 'opera';
  if (names.some((name: any) => name.indexOf('vivaldi') !== -1)) return 'vivaldi';
  if (names.some((name: any) => name.indexOf('chromium') !== -1)) return 'chromium';
  if (names.some((name: any) => name.indexOf('chrome') !== -1)) return 'chrome';

  const ua = String((nav && nav.userAgent) || '');
  if (/Edg\//.test(ua)) return 'edge';
  if (/OPR\//.test(ua)) return 'opera';
  if (/Chrome\//.test(ua)) return 'chrome';
  return 'unknown';
}

function computeCapabilities() {
  const capabilities = BASE_CAPABILITIES.slice();
  if (state.cdp && state.cdp.available === true) capabilities.push('cdp');
  return capabilities;
}

function cdpDescriptorForWire(result: any) {
  const value = result && typeof result === 'object' ? result : {};
  const available = value.available === true;
  return {
    available,
    endpoint: typeof value.endpoint === 'string' && value.endpoint ? value.endpoint : null,
    web_socket_debugger_url:
      typeof value.webSocketDebuggerUrl === 'string' && value.webSocketDebuggerUrl ? value.webSocketDebuggerUrl : null,
    browser: typeof value.browser === 'string' && value.browser ? value.browser : null,
    browser_family: typeof value.browserFamily === 'string' && value.browserFamily ? value.browserFamily : null,
    protocol_version: typeof value.protocolVersion === 'string' && value.protocolVersion ? value.protocolVersion : null,
    source: value.source === 'configured' ? 'configured' : 'auto',
    reason: available
      ? null
      : typeof value.reason === 'string' && value.reason
        ? value.reason
        : 'CDP endpoint unavailable',
  };
}

function cdpUiState() {
  if (!state.cdp) return null;
  const cdp = cdpDescriptorForWire(state.cdp);
  return {
    available: cdp.available,
    endpoint: cdp.endpoint,
    webSocketDebuggerUrl: cdp.web_socket_debugger_url,
    browser: cdp.browser,
    browserFamily: cdp.browser_family,
    expectedBrowserFamily: typeof state.cdp.expectedBrowserFamily === 'string' ? state.cdp.expectedBrowserFamily : null,
    familyMatch: state.cdp.familyMatch === true ? true : state.cdp.familyMatch === false ? false : null,
    protocolVersion: cdp.protocol_version,
    source: cdp.source,
    reason: cdp.reason,
    detected_at: state.cdpDetectedAt || null,
  };
}

function cdpFingerprint(result: any) {
  const cdp = cdpDescriptorForWire(result);
  return [
    cdp.available ? '1' : '0',
    cdp.endpoint || '',
    cdp.web_socket_debugger_url || '',
    cdp.browser || '',
    cdp.browser_family || '',
    cdp.protocol_version || '',
    cdp.source,
    cdp.reason || '',
  ].join('|');
}

async function refreshCdpDetection(reason: any, force: any): Promise<any> {
  if (cdpDetectionPromise) return cdpDetectionPromise;
  if (!force && state.cdp) return state.cdp;

  let source = state.cdpEndpoint === CDP_ENDPOINT_HELPER.DEFAULT_CDP_ENDPOINT ? 'auto' : 'configured';
  cdpDetectionPromise = (async () => {
    let result: any = null;
    try {
      const endpoint = await CDP_ENDPOINT_HELPER.readCdpEndpoint();
      state.cdpEndpoint = endpoint;
      source = endpoint === CDP_ENDPOINT_HELPER.DEFAULT_CDP_ENDPOINT ? 'auto' : 'configured';
      result = await CDP_ENDPOINT_HELPER.probe(endpoint);
    } catch (error) {
      result = {
        available: false,
        endpoint: null,
        webSocketDebuggerUrl: null,
        browser: null,
        protocolVersion: null,
        source,
        reason: 'CDP detection failed: ' + describeError(error),
      };
    }
    state.cdp = result;
    state.cdpDetectedAt = Date.now();
    log(
      'cdp detection',
      reason || 'refresh',
      result.available ? 'available' : 'unavailable',
      result.endpoint || '',
      result.reason || '',
    );
    return result;
  })().finally(() => {
    cdpDetectionPromise = null;
  });
  return cdpDetectionPromise;
}

async function ensureCdpDetection(force: any) {
  if (!force && state.cdp && !cdpDetectionPromise) return state.cdp;
  return refreshCdpDetection('ensure', force === true);
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

function fromContentError(error: any) {
  if (!error || typeof error !== 'object') {
    return new ToolError('INTERNAL', 'content script returned a malformed error object');
  }
  const code = typeof error.code === 'string' ? error.code : 'INTERNAL';
  const message =
    typeof error.message === 'string' && error.message ? error.message : 'content script reported an error';
  return new ToolError(code, message, error.details || null);
}

// ---------------------------------------------------------------------------
// State, storage, popup views
// ---------------------------------------------------------------------------

async function loadState() {
  const stored = await chrome.storage.local.get([
    STORAGE.connection,
    STORAGE.browserId,
    STORAGE.profile,
    STORAGE.daemonUrl,
    STORAGE.cdpEndpoint,
  ]);

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

  const daemonUrl = DAEMON_URL_HELPER.normalize(stored[STORAGE.daemonUrl]);
  state.wsUrl = daemonUrl || DAEMON_URL_HELPER.DEFAULT_DAEMON_URL;
  const cdpEndpoint = CDP_ENDPOINT_HELPER.normalize(stored[STORAGE.cdpEndpoint]);
  state.cdpEndpoint = cdpEndpoint || CDP_ENDPOINT_HELPER.DEFAULT_CDP_ENDPOINT;
  log('state loaded', state.status, state.profileToken, state.wsUrl, state.cdpEndpoint);
}

const stateReady = loadState().catch((error) => {
  log('state load failed', describeError(error));
});

async function persistConnection() {
  const snapshot = {
    status: state.status,
    browserId: state.browserId,
    daemonVersion: state.daemonVersion,
    lastError: state.lastError,
    updatedAt: Date.now(),
  };
  try {
    await chrome.storage.local.set({ [STORAGE.connection]: snapshot });
  } catch (error) {
    log('connection state could not be stored', describeError(error));
  }
}

function updateStatus(status: any, lastError: any) {
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
    lastError: state.lastError,
    lastPolicyDenied:
      state.lastPolicyDeniedAt === null ? null : { code: 'POLICY_DENIED', reason: state.lastPolicyDeniedReason },
    wsUrl: state.wsUrl || DAEMON_URL_HELPER.DEFAULT_DAEMON_URL,
    cdpEndpoint: state.cdpEndpoint,
    cdp: cdpUiState(),
    version: chrome.runtime.getManifest().version,
    capabilities: computeCapabilities(),
    debuggerChannel: {
      managed: true,
      attached_tabs: Array.from(debuggerSessions.keys()).map((tabId) => tabIdString(tabId)),
    },
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
      const contexts = await chrome.runtime.getContexts({ contextTypes: ['OFFSCREEN_DOCUMENT' as any] });
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
        justification:
          'Owns the loopback WebSocket and a heartbeat so the service worker is not suspended between commands.',
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

async function sendToOffscreen(message: any) {
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
  await ensureCdpDetection(false);
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
      cdp: cdpDescriptorForWire(state.cdp),
    },
  };
}

function buildEvent(name: any, payload: any) {
  return {
    v: PROTOCOL_VERSION,
    id: makeId(),
    kind: 'event',
    browser: state.browserId || undefined,
    tool: name,
    args: payload || {},
  };
}

async function sendCapabilitiesEvent(reason: any) {
  await ensureCdpDetection(false);
  const event = buildEvent('capabilities_changed', {
    capabilities: computeCapabilities(),
    cdp: cdpDescriptorForWire(state.cdp),
    reason: reason || 'local',
  });
  try {
    const reply = await sendToOffscreen({ type: 'corvus.send', frame: event });
    if (!reply || reply.ok !== true) log('capabilities event dropped: socket is closed');
  } catch (error) {
    log('capabilities event could not be sent', describeError(error));
  }
}

function buildResponse(frame: any, ok: any, result: any, error: any) {
  const traceId = frame && frame.meta && frame.meta.trace_id ? String(frame.meta.trace_id) : null;
  const response: any = {
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

async function sendFrame(frame: any) {
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

function handleHelloAck(frame: any) {
  const args = frame && frame.args && typeof frame.args === 'object' ? frame.args : {};
  const assigned = firstString(
    frame.browser,
    args.browser,
    args.browser_id,
    args.browserId,
    args.assigned_browser,
    args.assignedBrowser,
    args.id,
  );
  if (assigned) {
    state.browserId = assigned;
    chrome.storage.local.set({ [STORAGE.browserId]: assigned }).catch(() => {});
  }
  state.daemonVersion = firstString(args.daemon_version, args.daemonVersion, args.version, frame.version) || null;
  state.limits = args.limits !== undefined ? args.limits : null;
  state.lastPolicyDeniedReason = null;
  state.lastPolicyDeniedAt = null;
  updateStatus('connected', null);
  void persistConnection();
  log('hello_ack: browser=' + (state.browserId || 'unassigned') + ' daemon=' + (state.daemonVersion || 'unknown'));
}

function handleDaemonEvent(frame: any) {
  const args = frame && frame.args && typeof frame.args === 'object' ? frame.args : {};
  const name = firstString(args.event, args.name, args.type, frame && frame.tool);
  if (!name) return;

  if (name === 'cancel') {
    const commandId = firstString(args.command_id, args.commandId);
    if (!commandId) {
      log('ignoring cancel event without command_id');
      return;
    }
    if (cancelCommandById(commandId)) {
      log('cancel requested for command', commandId);
    } else {
      log('cancel event for unknown or finished command', commandId);
    }
    return;
  }

  if (name === 'capabilities_request' || name === 'hello_request') {
    void sendCapabilitiesEvent('event_request');
    return;
  }
  if (name === 'resume') {
    void sendToOffscreen({ type: 'corvus.connect', url: state.wsUrl, reason: 'daemon_resume' }).catch(() => {});
    return;
  }
  log('ignoring daemon event', name);
}

// ---------------------------------------------------------------------------
// Command routing
// ---------------------------------------------------------------------------

const HANDLERS: Record<string, (args: any, frame: any, signal?: any) => Promise<any>> = {
  browser_navigate: (args, frame, signal) => navigateTool(args, frame, signal),
  page_snapshot: (args, frame) => runInTab('page_snapshot', args, frame, { withTabId: true }),
  browser_click: (args, frame) => runInTab('browser_click', args, frame),
  browser_type: (args, frame) => runInTab('browser_type', args, frame),
  page_text: (args, frame) => runInTab('page_text', args, frame),
  page_html: (args, frame) => runInTab('page_html', args, frame),
  page_find: (args, frame) => runInTab('page_find', args, frame),
  browser_hover: (args, frame) => runInTab('browser_hover', args, frame),
  browser_press: (args, frame) => runInTab('browser_press', args, frame),
  browser_scroll: (args, frame) => runInTab('browser_scroll', args, frame),
  browser_select: (args, frame) => runInTab('browser_select', args, frame),
  browser_eval: (args, frame, signal) => evalTool(args, frame, signal),
  browser_wait: (args, frame, signal) => waitTool(args, frame, signal),
  browser_history: (args, frame) => historyTool(args, frame),
  tab_list: () => tabListTool(),
  tab_new: (args, frame) => tabNewTool(args, frame),
  tab_close: (args) => tabCloseTool(args),
  tab_switch: (args) => tabSwitchTool(args),
  net_capture: (args) => netCaptureTool(args),
  net_log: (args) => netLogTool(args),
  net_clear: (args) => netClearTool(args),
  net_intercept: (args) => netInterceptTool(args),
  download_list: (args) => downloadListTool(args),
  download_wait: (args, frame, signal) => downloadWaitTool(args, frame, signal),
  upload_file: (args) => uploadFileTool(args),
  browser_screenshot: (args) => screenshotTool(args),
  ...createExpandedHandlers({
    resolveTab: (args: any) => resolveTab(args),
    runInTab: (tool: any, args: any, frame: any, options?: any) => runInTab(tool, args, frame, options),
    requestContentResult: (tab: any, message: any) => requestContentResult(tab, message),
    sendCdpCommand: (tabId: any, method: string, params?: any) => sendCdpCommand(tabId, method, params),
    tabIdString: (tabId: any) => tabIdString(tabId),
  }),
};

function idempotencyKeyOf(frame: any) {
  const key = frame && frame.meta && frame.meta.idempotency_key;
  return typeof key === 'string' && key.trim() ? key.trim() : null;
}

function rememberResponse(key: any, response: any) {
  if (!key) return;
  responseCache.set(key, response);
  while (responseCache.size > RESPONSE_CACHE_LIMIT) {
    const oldest = responseCache.keys().next();
    if (oldest.done) break;
    responseCache.delete(oldest.value);
  }
}

function withTimeout(promise: any, timeoutMs: any) {
  let timer: ReturnType<typeof setTimeout> | null = null;
  const timeout = new Promise((resolve, reject) => {
    timer = setTimeout(() => {
      reject(new ToolError('TIMEOUT', 'command exceeded ' + timeoutMs + ' ms'));
    }, timeoutMs);
  });
  return Promise.race([promise, timeout]).finally(() => {
    if (timer !== null) clearTimeout(timer);
  });
}

function cancelledError() {
  return new ToolError('CANCELLED', 'command was cancelled');
}

function throwIfCancelled(signal: any) {
  if (signal && signal.aborted) throw cancelledError();
}

// Race a handler against cooperative abort so even a short non-cooperative
// handler answers the daemon promptly. The handler may keep running after the
// response was sent; the daemon has already dropped the cancelled command.
function raceWithAbort(promise: any, signal: any) {
  if (!signal) return promise;
  if (signal.aborted) return Promise.reject(cancelledError());
  return new Promise((resolve, reject) => {
    let settled = false;
    const cleanup = () => signal.removeEventListener('abort', onAbort);
    const onAbort = () => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(cancelledError());
    };
    signal.addEventListener('abort', onAbort, { once: true });
    promise.then(
      (value: any) => {
        if (settled) return;
        settled = true;
        cleanup();
        resolve(value);
      },
      (error: any) => {
        if (settled) return;
        settled = true;
        cleanup();
        reject(error);
      },
    );
  });
}

function cancelCommandById(commandId: any) {
  const id = commandId === undefined || commandId === null ? '' : String(commandId);
  if (id.trim() === '') return false;
  const controller = commandControllers.get(id);
  if (!controller) return false;
  controller.abort();
  return true;
}

function commandTimeoutFor(tool: any, args: any, frame: any) {
  let timeoutMs = clampTimeout(frame && frame.meta ? frame.meta.timeout_ms : undefined);
  // download_wait may legitimately outlive the daemon default: its own argument
  // defines the wait and the command budget is that wait plus a small margin.
  if (tool === 'download_wait') {
    const requested = args && args.timeout_ms !== undefined && args.timeout_ms !== null ? args.timeout_ms : 30000;
    if (Number.isInteger(requested) && requested >= 100 && requested <= 120000) {
      timeoutMs = Math.min(MAX_TIMEOUT_MS, Math.max(timeoutMs, requested + 5000));
    }
  }
  return timeoutMs;
}

async function handleCommand(frame: any) {
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

  const commandId = frame && frame.id !== undefined && frame.id !== null ? String(frame.id) : '';
  const controller = new AbortController();
  if (commandId) {
    commandControllers.set(commandId, controller);
    while (commandControllers.size > MAX_COMMAND_CONTROLLERS) {
      const oldestId = commandControllers.keys().next().value;
      if (oldestId === undefined) break;
      const oldest = commandControllers.get(oldestId);
      commandControllers.delete(oldestId);
      if (oldest) oldest.abort();
    }
  }

  let response;
  try {
    const rawTool = firstString(frame && frame.tool);
    if (!rawTool) throw new ToolError('BAD_ARGS', 'command envelope is missing tool');
    const tool = rawTool.toLowerCase();
    if (TOOL_NAMES.indexOf(tool) === -1)
      throw new ToolError('BAD_ARGS', 'unknown tool: ' + rawTool, { tool: rawTool, known: TOOL_NAMES });

    const handler = HANDLERS[tool];
    if (typeof handler !== 'function') throw new ToolError('INTERNAL', 'no handler registered for ' + tool);

    const args = frame.args && typeof frame.args === 'object' ? frame.args : {};
    await assertBrowserSelection(args);
    const timeoutMs = commandTimeoutFor(tool, args, frame);
    const result = await withTimeout(
      raceWithAbort(
        Promise.resolve().then(() => {
          throwIfCancelled(controller.signal);
          return handler(args, frame, controller.signal);
        }),
        controller.signal,
      ),
      timeoutMs + COMMAND_TIMEOUT_PAD_MS,
    );
    state.lastPolicyDeniedReason = null;
    state.lastPolicyDeniedAt = null;
    response = buildResponse(frame, true, result, null);
  } catch (error) {
    const payload = toErrorPayload(error, traceId);
    if (payload.code === 'POLICY_DENIED') {
      state.lastPolicyDeniedReason = policyDeniedReasonFromDetails(payload.details);
      state.lastPolicyDeniedAt = Date.now();
    }
    response = buildResponse(frame, false, null, payload);
  } finally {
    if (commandId && commandControllers.get(commandId) === controller) {
      commandControllers.delete(commandId);
    }
  }

  rememberResponse(key, response);
  log('command', (frame && frame.tool) || '?', 'answered in', Date.now() - startedAt, 'ms');
  await sendFrame(response);
}

async function handleInboundFrame(frame: any) {
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
      await sendFrame({
        v: PROTOCOL_VERSION,
        id: String(frame.id || makeId()),
        kind: 'pong',
        browser: state.browserId || undefined,
      });
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

const RESTRICTED_PROTOCOLS = [
  'chrome:',
  'chrome-extension:',
  'chrome-untrusted:',
  'devtools:',
  'edge:',
  'brave:',
  'opera:',
  'vivaldi:',
  'view-source:',
  'about:',
];

function isRestrictedUrl(url: any) {
  if (!url) return false;
  const lowered = String(url).trim().toLowerCase();
  if (lowered === '' || lowered === 'about:blank') return false;
  return RESTRICTED_PROTOCOLS.some((protocol) => lowered.startsWith(protocol));
}

async function resolveTab(args: any) {
  const raw = args ? (args.tab_id !== undefined ? args.tab_id : args.tabId) : undefined;
  if (raw !== undefined && raw !== null && String(raw).trim() !== '') {
    // The protocol documents tab_id as a string, while chrome.tabs uses a
    // number. Accept both forms, but a string that is not made of digits is a
    // well-formed id for a tab that does not exist, not a schema violation.
    let tabId = null;
    if (typeof raw === 'number') {
      if (!Number.isInteger(raw) || raw < 0) {
        throw new ToolError('BAD_ARGS', 'args.tab_id must be a tab id string or non-negative integer', { tab_id: raw });
      }
      tabId = raw;
    } else if (typeof raw === 'string') {
      const text = raw.trim();
      if (!/^\d+$/.test(text)) {
        throw new ToolError('NOT_FOUND', 'tab ' + text + ' does not exist', { tab_id: raw });
      }
      tabId = Number(text);
      if (!Number.isSafeInteger(tabId) || tabId < 0) {
        throw new ToolError('NOT_FOUND', 'tab ' + text + ' does not exist', { tab_id: raw });
      }
    } else {
      throw new ToolError('BAD_ARGS', 'args.tab_id must be a tab id string or non-negative integer', { tab_id: raw });
    }
    try {
      return await chrome.tabs.get(tabId);
    } catch (error) {
      throw new ToolError('NOT_FOUND', 'tab ' + tabId + ' does not exist', { tab_id: raw });
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

function isNoReceiverError(error: any) {
  return /Receiving end does not exist|Could not establish connection/i.test(describeError(error));
}

function isPortClosedError(error: any) {
  return /message port closed|Could not establish connection/i.test(describeError(error));
}

function classifyInjectionFailure(error: any) {
  const message = describeError(error);
  if (/Cannot access|extensions gallery cannot be scripted|cannot be scripted/i.test(message)) {
    return new ToolError('CAPABILITY_UNAVAILABLE', 'this page cannot be scripted: ' + message, {
      reason: 'restricted_scheme',
    });
  }
  if (/was closed|has been closed|Frame with ID .* was removed/i.test(message)) {
    return new ToolError('BROWSER_GONE', 'the tab went away before the command could run', { reason: 'tab_closed' });
  }
  if (/No tab with id|Invalid tab ID/i.test(message)) {
    return new ToolError('NOT_FOUND', 'the target tab does not exist', { reason: 'tab_missing' });
  }
  return new ToolError('INTERNAL', 'content script injection failed: ' + message);
}

async function ensureContentScript(tabId: any, force: any) {
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

async function sendContentCommand(tabId: any, message: any) {
  try {
    return await chrome.tabs.sendMessage(tabId, message);
  } catch (error) {
    if (!isNoReceiverError(error)) throw error;
    await ensureContentScript(tabId, true);
    return chrome.tabs.sendMessage(tabId, message);
  }
}

function classifyContentFailure(error: any, tab: any) {
  const message = describeError(error);
  if (/message port closed|The page was reloaded|navigated/i.test(message)) {
    return new ToolError('NAVIGATION_LOST', 'the page navigated before the command answered', {
      url: tab && tab.url ? tab.url : null,
    });
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

async function runInTab(tool: any, args: any, frame: any, options?: any) {
  const tab = await resolveTab(args);
  if (!tab || typeof tab.id !== 'number') {
    throw new ToolError('NOT_FOUND', 'no active tab is available');
  }
  if (isRestrictedUrl(tab.url)) {
    throw new ToolError('CAPABILITY_UNAVAILABLE', 'this page cannot be scripted: ' + String(tab.url), {
      url: tab.url,
      reason: 'restricted_scheme',
    });
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
  if (
    options &&
    options.withTabId === true &&
    reply.result &&
    typeof reply.result === 'object' &&
    !Array.isArray(reply.result)
  ) {
    return Object.assign({ tab_id: tabIdString(tab.id) }, reply.result);
  }
  return reply.result;
}

// Sends a non-tool message to a content script and unwraps its reply. Used by
// upload_file to resolve a hidden file input through the page DOM first.
async function requestContentResult(tab: any, message: any) {
  if (!tab || typeof tab.id !== 'number') {
    throw new ToolError('NOT_FOUND', 'no tab is available for the content request');
  }
  await ensureContentScript(tab.id, false);
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

async function screenshotTool(args: any) {
  const tab = await resolveTab(args);
  if (!tab || typeof tab.id !== 'number') {
    throw new ToolError('NOT_FOUND', 'no active tab is available');
  }
  if (isRestrictedUrl(tab.url)) {
    throw new ToolError('CAPABILITY_UNAVAILABLE', 'this page cannot be captured: ' + String(tab.url), {
      url: tab.url,
      reason: 'restricted_scheme',
    });
  }
  return captureScreenshotImage(
    args,
    {
      id: tab.id,
      url: tab.url,
      windowId: tab.windowId,
      active: tab.active,
    },
    {
      sendCdp: (tabId: number, method: string, params: any) => sendCdpCommand(tabId, method, params),
      captureVisibleTab: (windowId?: number) =>
        windowId === undefined
          ? chrome.tabs.captureVisibleTab({ format: 'png' })
          : chrome.tabs.captureVisibleTab(windowId, { format: 'png' }),
      resolveElementRect: async (tabId: number, ref: string) => {
        const result = await requestContentResult(tab, { type: 'corvus.element_rect', ref: ref });
        if (!result || typeof result !== 'object') {
          throw new ToolError('INTERNAL', 'content script returned no element rectangle');
        }
        return result as ScreenshotElementRect;
      },
    },
  );
}

// ---------------------------------------------------------------------------
// Helpers: tab ids, bounded arguments, tab-load watches
// ---------------------------------------------------------------------------

function sleep(ms: any) {
  return new Promise((resolve) => {
    setTimeout(resolve, Math.max(0, ms));
  });
}

function tabIdString(tabId: any) {
  return String(tabId);
}

function requireTabId(args: any, tool: any) {
  const raw = args ? args.tab_id : undefined;
  if (raw === undefined || raw === null || String(raw).trim() === '') {
    throw new ToolError('BAD_ARGS', tool + ' requires a non-empty tab_id argument');
  }
}

function actionBudgetMs(frame: any) {
  const timeoutMs = clampTimeout(frame && frame.meta ? frame.meta.timeout_ms : undefined);
  return Math.max(0, Math.min(timeoutMs - 2000, 15000));
}

function readBoundedInteger(value: any, fallback: any, min: any, max: any, label: any) {
  if (value === undefined || value === null) return fallback;
  const numeric = Number(value);
  if (!Number.isInteger(numeric) || numeric < min || numeric > max) {
    throw new ToolError('BAD_ARGS', label + ' must be an integer from ' + min + ' to ' + max, { value: value });
  }
  return numeric;
}

function readWaitCondition(args: any, key: any) {
  const value = args ? args[key] : undefined;
  if (value === undefined || value === null) return null;
  if (typeof value !== 'string' || value.trim() === '') {
    throw new ToolError('BAD_ARGS', 'args.' + key + ' must be a non-empty string', { key: key });
  }
  return key === 'selector' ? value.trim() : value;
}

function parseNewTabUrl(raw: any) {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== 'string' || raw.trim() === '') {
    throw new ToolError('BAD_ARGS', 'args.url must be a non-empty string when provided');
  }
  let resolved = null;
  try {
    resolved = new URL(raw.trim());
  } catch (error) {
    throw new ToolError('BAD_ARGS', 'args.url is not a valid absolute URL', { url: raw.slice(0, 200) });
  }
  if (!/^(https?|file):$/i.test(resolved.protocol)) {
    throw new ToolError('BAD_ARGS', 'only http, https and file URLs can be opened in a new tab', {
      url: resolved.href.slice(0, 200),
    });
  }
  return resolved.href;
}

function watchTabLoad(
  tabId: any,
  beforeUrl: any,
  action: any,
  timeoutMs: any,
): { promise: Promise<{ loaded: boolean; url: string | null }>; cancel: () => void } {
  let cancelWatch: (() => void) | null = null;
  const promise = new Promise<{ loaded: boolean; url: string | null }>((resolve) => {
    let settled = false;
    let sawLoading = false;
    let timeoutTimer: ReturnType<typeof setTimeout> | null = null;
    let pollTimer: ReturnType<typeof setTimeout> | null = null;

    function cleanup() {
      try {
        chrome.tabs.onUpdated.removeListener(onUpdated);
      } catch (error) {
        // Ignore listener cleanup failures.
      }
      if (timeoutTimer !== null) {
        clearTimeout(timeoutTimer);
        timeoutTimer = null;
      }
      if (pollTimer !== null) {
        clearTimeout(pollTimer);
        pollTimer = null;
      }
    }

    function finish(loaded: any, url: any) {
      if (settled) return;
      settled = true;
      cleanup();
      resolve({ loaded: loaded === true, url: url || null });
    }

    function onUpdated(updatedTabId: any, changeInfo: any, tab: any) {
      if (settled || updatedTabId !== tabId) return;
      if (!changeInfo) return;
      if (changeInfo.status === 'loading') {
        sawLoading = true;
        return;
      }
      if (changeInfo.status !== 'complete') return;
      const url = (tab && tab.url) || changeInfo.url || null;
      if (action === 'reload') {
        if (sawLoading || (url && beforeUrl && url !== beforeUrl)) finish(true, url);
        return;
      }
      if (sawLoading || !beforeUrl || (url && url !== beforeUrl)) finish(true, url);
    }

    cancelWatch = function cancelWatch() {
      finish(false, beforeUrl);
    };

    chrome.tabs.onUpdated.addListener(onUpdated);

    timeoutTimer = setTimeout(
      async () => {
        if (settled) return;
        let url = beforeUrl;
        try {
          const tab = await chrome.tabs.get(tabId);
          url = tab && tab.url ? tab.url : beforeUrl;
        } catch (error) {
          url = beforeUrl;
        }
        finish(false, url);
      },
      Math.max(1, timeoutMs),
    );

    // A fast load may finish before the 100 ms probe; the listener is already
    // attached, so complete events are never missed.
    pollTimer = setTimeout(() => {
      if (settled) return;
      chrome.tabs
        .get(tabId)
        .then((tab) => {
          if (settled || !tab) return;
          if (tab.status === 'loading') {
            sawLoading = true;
            return;
          }
          if (tab.status !== 'complete') return;
          if (action === 'reload') {
            if (sawLoading) finish(true, tab.url || null);
            return;
          }
          if (!beforeUrl || (tab.url && tab.url !== beforeUrl) || sawLoading) finish(true, tab.url || null);
        })
        .catch(() => {
          // The timeout path reports the final state.
        });
    }, 100);
  });

  return {
    promise: promise,
    cancel: function cancel() {
      if (typeof cancelWatch === 'function') cancelWatch();
    },
  };
}

async function historyTool(args: any, frame: any) {
  const rawAction = args && typeof args.action === 'string' ? args.action.trim().toLowerCase() : '';
  if (rawAction !== 'back' && rawAction !== 'forward' && rawAction !== 'reload') {
    throw new ToolError('BAD_ARGS', 'args.action must be back, forward or reload', { action: rawAction || null });
  }

  const tab = await resolveTab(args);
  if (!tab || typeof tab.id !== 'number') {
    throw new ToolError('NOT_FOUND', 'no active tab is available');
  }

  const beforeUrl = tab.url || null;
  const budget = actionBudgetMs(frame);
  let outcome: { loaded: boolean; url: string | null } = { loaded: false, url: beforeUrl };

  if (rawAction === 'reload') {
    const watch = budget > 0 ? watchTabLoad(tab.id, beforeUrl, rawAction, budget) : null;
    try {
      await chrome.tabs.reload(tab.id);
    } catch (error) {
      if (watch) watch.cancel();
      const message = describeError(error);
      if (/No tab with id|Invalid tab ID/i.test(message)) {
        throw new ToolError('BROWSER_GONE', 'the tab was closed before history could change', { reason: 'tab_closed' });
      }
      throw new ToolError('INTERNAL', 'history reload failed: ' + message);
    }
    if (watch) outcome = await watch.promise;
  } else {
    // The tabs API back and forward commands walk the browser's session
    // history, which can skip entries created by earlier tool calls. The CDP
    // navigation history is the page's real entry list, so pick the adjacent
    // entry by index and ask the browser to activate exactly that entry.
    let history: any = null;
    try {
      history = await sendCdpCommand(tab.id, 'Page.getNavigationHistory', {});
    } catch (error) {
      if (error instanceof ToolError) throw error;
      throw new ToolError('INTERNAL', 'browser_history could not read the navigation history: ' + describeError(error));
    }

    const entries = history && Array.isArray(history.entries) ? history.entries : [];
    const currentIndex = history && Number.isInteger(history.currentIndex) ? history.currentIndex : -1;
    const targetIndex = currentIndex + (rawAction === 'back' ? -1 : 1);
    if (targetIndex < 0 || targetIndex >= entries.length) {
      throw new ToolError('NOT_FOUND', 'no ' + rawAction + ' history entry is available', { action: rawAction });
    }

    const targetEntry = entries[targetIndex];
    if (!targetEntry || targetEntry.id === undefined || targetEntry.id === null) {
      throw new ToolError('INTERNAL', 'the ' + rawAction + ' history entry has no id', { action: rawAction });
    }

    const watch = budget > 0 ? watchTabLoad(tab.id, beforeUrl, rawAction, budget) : null;
    try {
      await sendCdpCommand(tab.id, 'Page.navigateToHistoryEntry', { entryId: targetEntry.id });
    } catch (error) {
      if (watch) watch.cancel();
      if (error instanceof ToolError && error.code === 'CAPABILITY_UNAVAILABLE') throw error;
      const message = describeError(error);
      if (/Cannot find|No entry|entry.*not found|not found|invalid/i.test(message)) {
        throw new ToolError('NOT_FOUND', 'no ' + rawAction + ' history entry is available', { action: rawAction });
      }
      if (error instanceof ToolError) throw error;
      throw new ToolError('INTERNAL', 'history ' + rawAction + ' failed: ' + message);
    }
    if (watch) outcome = await watch.promise;
  }

  let current = null;
  try {
    current = await chrome.tabs.get(tab.id);
  } catch (error) {
    throw new ToolError('BROWSER_GONE', 'the tab was closed while history was changing', { reason: 'tab_closed' });
  }

  return {
    action: rawAction,
    url: (current && current.url) || outcome.url || beforeUrl || '',
    loaded: outcome.loaded === true,
  };
}

async function probeWaitContent(tabId: any, selector: any, text: any) {
  const message: any = { type: 'corvus.probe' };
  if (selector !== null) message.selector = selector;
  if (text !== null) message.text = text;
  const reply = await sendContentCommand(tabId, message);
  if (!reply || typeof reply !== 'object') {
    throw new ToolError('INTERNAL', 'content script returned an empty wait probe');
  }
  if (reply.ok === false) throw fromContentError(reply.error);
  return reply;
}

async function waitTool(args: any, frame: any, signal: any) {
  const selector = readWaitCondition(args, 'selector');
  const text = readWaitCondition(args, 'text');
  const urlContains = readWaitCondition(args, 'url_contains');
  if (selector === null && text === null && urlContains === null) {
    throw new ToolError('BAD_ARGS', 'browser_wait needs at least one of selector, text or url_contains');
  }

  const requested = readBoundedInteger(args ? args.timeout_ms : undefined, 10000, 100, 120000, 'args.timeout_ms');
  const tab = await resolveTab(args);
  if (!tab || typeof tab.id !== 'number') {
    throw new ToolError('NOT_FOUND', 'no active tab is available');
  }
  if ((selector !== null || text !== null) && isRestrictedUrl(tab.url)) {
    throw new ToolError('CAPABILITY_UNAVAILABLE', 'this page cannot be scripted: ' + String(tab.url), {
      url: tab.url,
      reason: 'restricted_scheme',
    });
  }

  const commandTimeout = clampTimeout(frame && frame.meta ? frame.meta.timeout_ms : undefined);
  const budget = Math.max(100, Math.min(requested, commandTimeout - 500));
  const startedAt = Date.now();

  while (true) {
    throwIfCancelled(signal);
    let current = null;
    try {
      current = await chrome.tabs.get(tab.id);
    } catch (error) {
      throw new ToolError('BROWSER_GONE', 'the target tab was closed while waiting', { reason: 'tab_closed' });
    }

    const currentUrl = current.url || '';
    let matched = true;
    if (urlContains !== null && currentUrl.indexOf(urlContains) === -1) matched = false;

    if (matched && (selector !== null || text !== null)) {
      try {
        const probe = await probeWaitContent(tab.id, selector, text);
        if (probe.matched !== true) matched = false;
      } catch (error) {
        if (signal && signal.aborted) throw cancelledError();
        const mapped = error instanceof ToolError ? error : classifyContentFailure(error, current);
        if (
          mapped.code === 'BAD_ARGS' ||
          mapped.code === 'CAPABILITY_UNAVAILABLE' ||
          mapped.code === 'BROWSER_GONE' ||
          mapped.code === 'NOT_FOUND'
        ) {
          throw mapped;
        }
        matched = false;
      }
    }

    if (matched) {
      return {
        matched: true,
        waited_ms: Date.now() - startedAt,
        url: currentUrl,
      };
    }

    const elapsed = Date.now() - startedAt;
    if (elapsed >= budget) {
      throw new ToolError('TIMEOUT', 'browser_wait timed out after ' + elapsed + ' ms', { waited_ms: elapsed });
    }
    await sleep(Math.min(100, Math.max(10, budget - elapsed)));
    throwIfCancelled(signal);
  }
}

function isCdpSerializationError(message: any) {
  return /(could not|couldn't) be returned by value|object reference chain|circular|not serializ|serializ(e|ation) failed/i.test(
    String(message),
  );
}

function remoteObjectToEvalResult(remote: any) {
  if (!remote || typeof remote !== 'object') {
    return { ok: true, value: null, value_note: 'unserializable' };
  }
  if (typeof remote.description === 'string' && isCdpSerializationError(remote.description)) {
    return { ok: true, value: null, value_note: 'unserializable' };
  }
  if (Object.prototype.hasOwnProperty.call(remote, 'value')) {
    return { ok: true, value: remote.value === undefined ? null : remote.value };
  }
  if (remote.subtype === 'null') {
    return { ok: true, value: null };
  }
  // JSON has no representation for undefined, functions, symbols or BigInt.
  // NaN and Infinity become null through the JSON round trip, so report them
  // as null without the unserializable note, matching the previous contract.
  const unserializable = remote.unserializableValue;
  if (
    unserializable === 'NaN' ||
    unserializable === 'Infinity' ||
    unserializable === '-Infinity' ||
    unserializable === '-0'
  ) {
    return { ok: true, value: null };
  }
  return { ok: true, value: null, value_note: 'unserializable' };
}

function exceptionDetailsToEvalResult(details: any) {
  const exception = details && details.exception ? details.exception : {};
  let name = exception.className ? String(exception.className) : '';
  let message = '';
  if (typeof exception.description === 'string' && exception.description !== '') {
    message = exception.description;
  } else if (exception.value !== undefined) {
    message = String(exception.value);
  } else if (details && details.text) {
    message = String(details.text);
  }
  if (!name) name = 'Error';
  if (message.indexOf(name + ': ') === 0) {
    message = message.slice(name.length + 2);
  } else if (message.indexOf(name + ':') === 0) {
    message = message.slice(name.length + 1);
  }
  const newline = message.indexOf('\n');
  if (newline !== -1) message = message.slice(0, newline);
  message = message.trim();
  if (message === '') message = details && details.text ? String(details.text) : name;
  return { ok: false, error: { name: name, message: message } };
}

function buildEvalResult(response: any) {
  if (!response || typeof response !== 'object') {
    throw new ToolError('INTERNAL', 'Runtime.evaluate returned no response');
  }
  if (response.exceptionDetails) {
    return exceptionDetailsToEvalResult(response.exceptionDetails);
  }
  return remoteObjectToEvalResult(response.result);
}

async function createIsolatedEvalContext(tabId: any) {
  const tree: any = await sendCdpCommand(tabId, 'Page.getFrameTree', {});
  const frame = tree && tree.frameTree && tree.frameTree.frame;
  if (!frame || typeof frame.id !== 'string' || frame.id === '') {
    throw new ToolError('INTERNAL', 'browser_eval could not resolve the main frame', { tab_id: tabIdString(tabId) });
  }
  const world: any = await sendCdpCommand(tabId, 'Page.createIsolatedWorld', {
    frameId: frame.id,
    worldName: 'corvus-eval',
  });
  const contextId = world && world.executionContextId;
  if (!Number.isInteger(contextId) || contextId < 0) {
    throw new ToolError('INTERNAL', 'browser_eval could not create an isolated world', { tab_id: tabIdString(tabId) });
  }
  return contextId;
}

async function evalTool(args: any, frame: any, signal: any) {
  const expression = args ? args.expression : undefined;
  if (typeof expression !== 'string' || expression.trim() === '') {
    throw new ToolError('BAD_ARGS', 'args.expression must be a non-empty string');
  }

  let world = 'isolated';
  if (args.world !== undefined && args.world !== null) {
    if (typeof args.world !== 'string') {
      throw new ToolError('BAD_ARGS', 'args.world must be "main" or "isolated"');
    }
    const requestedWorld = args.world.trim().toLowerCase();
    if (requestedWorld !== 'main' && requestedWorld !== 'isolated') {
      throw new ToolError('BAD_ARGS', 'args.world must be "main" or "isolated"', { world: args.world });
    }
    world = requestedWorld;
  }

  const tab = await resolveTab(args);
  if (!tab || typeof tab.id !== 'number') {
    throw new ToolError('NOT_FOUND', 'no active tab is available');
  }
  if (isRestrictedUrl(tab.url)) {
    throw new ToolError('CAPABILITY_UNAVAILABLE', 'this page cannot be scripted: ' + String(tab.url), {
      url: tab.url,
      reason: 'restricted_scheme',
    });
  }

  let terminateRequested = false;
  const terminateExecution = () => {
    if (terminateRequested) return;
    terminateRequested = true;
    void sendCdpCommand(tab.id, 'Runtime.terminateExecution', {}).catch((error) => {
      log('Runtime.terminateExecution failed', describeError(error));
    });
  };
  if (signal) {
    if (signal.aborted) {
      terminateExecution();
      throw cancelledError();
    }
    signal.addEventListener('abort', terminateExecution, { once: true });
  }

  // Runtime.evaluate runs the string as page code, so MV3's extension CSP does
  // not apply. The automatic debugger channel is the only execution path here;
  // scripting is not required for browser_eval anymore.
  const params: any = {
    expression: expression,
    awaitPromise: true,
    returnByValue: true,
    userGesture: true,
  };

  try {
    const evaluation = (async () => {
      if (world === 'isolated') {
        params.contextId = await createIsolatedEvalContext(tab.id);
      }
      return sendCdpCommand(tab.id, 'Runtime.evaluate', params);
    })();
    const response = await raceWithAbort(evaluation, signal);
    throwIfCancelled(signal);
    return buildEvalResult(response);
  } catch (error) {
    if (signal && signal.aborted) throw cancelledError();
    if (error instanceof ToolError && error.code === 'CAPABILITY_UNAVAILABLE') throw error;
    const message = describeError(error);
    if (isCdpSerializationError(message)) {
      return { ok: true, value: null, value_note: 'unserializable' };
    }
    if (error instanceof ToolError) throw error;
    throw new ToolError('INTERNAL', 'browser_eval failed: ' + message);
  } finally {
    if (signal) signal.removeEventListener('abort', terminateExecution);
  }
}

async function tabListTool() {
  const tabs = await chrome.tabs.query({});
  const ordered = (tabs || []).slice().sort((left, right) => {
    if (left.windowId !== right.windowId) return left.windowId - right.windowId;
    return left.index - right.index;
  });
  return {
    tabs: ordered.map((tab) => ({
      tab_id: tabIdString(tab.id),
      index: tab.index,
      active: tab.active === true,
      url: tab.url || '',
      title: tab.title || '',
      status: tab.status || 'unloaded',
    })),
  };
}

async function tabNewTool(args: any, frame: any) {
  const url = parseNewTabUrl(args ? args.url : undefined);
  if (args && args.active !== undefined && args.active !== null && typeof args.active !== 'boolean') {
    throw new ToolError('BAD_ARGS', 'args.active must be a boolean');
  }
  const active = !(args && args.active === false);

  let previousTabId = null;
  try {
    const current = await resolveTab({});
    if (current && typeof current.id === 'number') previousTabId = tabIdString(current.id);
  } catch (error) {
    previousTabId = null;
  }

  const createProperties: any = { active: active };
  if (url) createProperties.url = url;

  let created = null;
  try {
    created = await chrome.tabs.create(createProperties);
  } catch (error) {
    throw new ToolError('INTERNAL', 'the tab could not be created: ' + describeError(error));
  }
  if (!created || typeof created.id !== 'number') {
    throw new ToolError('INTERNAL', 'chrome.tabs.create returned no tab id');
  }

  const budget = actionBudgetMs(frame);
  let outcome: any = null;
  if (budget > 0) {
    outcome = await waitForNewTabLoad(created.id, url || null, beginNavigationEpoch(created.id), budget);
  }

  let current = created;
  try {
    current = await chrome.tabs.get(created.id);
  } catch (error) {
    throw new ToolError('BROWSER_GONE', 'the new tab disappeared before it could be reported', {
      reason: 'tab_closed',
    });
  }

  return {
    tab_id: tabIdString(created.id),
    url: (current && current.url) || url || '',
    loaded: outcome ? outcome.loaded === true : current && current.status === 'complete',
    previous_tab_id: previousTabId,
  };
}

async function tabCloseTool(args: any) {
  requireTabId(args, 'tab_close');
  const tab = await resolveTab(args);
  if (!tab || typeof tab.id !== 'number') {
    throw new ToolError('NOT_FOUND', 'the target tab does not exist');
  }

  let neighbour = null;
  try {
    const siblings = await chrome.tabs.query({ windowId: tab.windowId });
    const ordered = (siblings || [])
      .filter((item) => item.id !== tab.id)
      .sort((left, right) => left.index - right.index);
    const after = ordered.find((item) => item.index > tab.index);
    if (after) {
      neighbour = after;
    } else if (ordered.length > 0) {
      neighbour = ordered[ordered.length - 1];
    }
  } catch (error) {
    neighbour = null;
  }

  try {
    await chrome.tabs.remove(tab.id);
  } catch (error) {
    const message = describeError(error);
    if (/No tab with id|Invalid tab ID/i.test(message)) {
      throw new ToolError('NOT_FOUND', 'the tab no longer exists', { tab_id: tabIdString(tab.id) });
    }
    throw new ToolError('INTERNAL', 'the tab could not be closed: ' + message);
  }

  const result: any = { closed: true, tab_id: tabIdString(tab.id) };
  if (tab.active === true && neighbour) result.next_tab_id = tabIdString(neighbour.id);
  return result;
}

async function tabSwitchTool(args: any) {
  requireTabId(args, 'tab_switch');
  const tab = await resolveTab(args);
  if (!tab || typeof tab.id !== 'number') {
    throw new ToolError('NOT_FOUND', 'the target tab does not exist');
  }

  try {
    await chrome.tabs.update(tab.id, { active: true });
  } catch (error) {
    const message = describeError(error);
    if (/No tab with id|Invalid tab ID/i.test(message)) {
      throw new ToolError('NOT_FOUND', 'the target tab no longer exists', { tab_id: tabIdString(tab.id) });
    }
    throw new ToolError('INTERNAL', 'the tab could not be activated: ' + message);
  }

  try {
    if (chrome.windows && typeof chrome.windows.update === 'function') {
      await chrome.windows.update(tab.windowId, { focused: true });
    }
  } catch (error) {
    // Activating the tab is enough; window focus is best effort.
  }

  let current = tab;
  try {
    current = await chrome.tabs.get(tab.id);
  } catch (error) {
    throw new ToolError('BROWSER_GONE', 'the tab disappeared after activation', { reason: 'tab_closed' });
  }

  return {
    tab_id: tabIdString(current.id),
    url: current.url || '',
    title: current.title || '',
    active: true,
  };
}

// ---------------------------------------------------------------------------
// Browser routing, network capture, interception, downloads and uploads
// ---------------------------------------------------------------------------

// Every tool accepts an optional browser_id. This extension owns exactly one
// transport, so a value that is present must name the browser assigned by the
// daemon in hello_ack; anything else is TRANSPORT_DOWN. Omitted is allowed.
async function assertBrowserSelection(args: any) {
  const raw = args ? args.browser_id : undefined;
  if (raw === undefined || raw === null) return;
  if (typeof raw !== 'string' || raw.trim() === '') {
    throw new ToolError('BAD_ARGS', 'args.browser_id must be a non-empty string when provided');
  }
  const requested = raw.trim();
  await stateReady;
  if (!state.browserId || String(state.browserId) !== requested) {
    throw new ToolError('TRANSPORT_DOWN', 'browser ' + requested + ' is not connected to this extension', {
      browser_id: requested,
    });
  }
}

function hasOwn(object: any, key: any) {
  return Object.prototype.hasOwnProperty.call(object, key);
}

function hasTabIdArgument(args: any) {
  const raw = args ? args.tab_id : undefined;
  return raw !== undefined && raw !== null && String(raw).trim() !== '';
}

function readOptionalString(value: any, label: any) {
  if (value === undefined || value === null) return null;
  if (typeof value !== 'string' || value.trim() === '') {
    throw new ToolError('BAD_ARGS', label + ' must be a non-empty string', { value: value });
  }
  return value.trim();
}

function readBoolean(value: any, fallback: any, label: any) {
  if (value === undefined || value === null) return fallback;
  if (typeof value !== 'boolean') {
    throw new ToolError('BAD_ARGS', label + ' must be a boolean', { value: value });
  }
  return value;
}

function isAbsoluteFilePath(value: any) {
  if (typeof value !== 'string' || value.trim() === '') return false;
  const path = value.trim();
  if (path.indexOf('\u0000') !== -1) return false;
  if (/^[A-Za-z]:[\\/]/.test(path)) return true;
  if (path.indexOf('\\\\') === 0) return true;
  return path.charAt(0) === '/' || path.charAt(0) === '\\';
}

function parseDownloadId(value: any) {
  if (value === undefined || value === null) return null;
  if (typeof value === 'number') {
    if (!Number.isInteger(value) || value < 0) {
      throw new ToolError('BAD_ARGS', 'args.id must be a non-negative integer download id', { id: value });
    }
    return value;
  }
  if (typeof value === 'string' && /^\d+$/.test(value.trim())) {
    const numeric = Number(value.trim());
    if (Number.isSafeInteger(numeric) && numeric >= 0) return numeric;
  }
  throw new ToolError('BAD_ARGS', 'args.id must be a non-negative integer download id', { id: value });
}

// ---------------------------------------------------------------------------
// Network capture state and pure helpers
// ---------------------------------------------------------------------------

function cloneHeaderMap(headers: any) {
  const output: any = {};
  if (!headers || typeof headers !== 'object') return output;
  Object.keys(headers).forEach((name) => {
    const value = headers[name];
    if (typeof value === 'string') output[name] = value;
    else if (value !== undefined && value !== null) output[name] = String(value);
  });
  return output;
}

function durationFromTimestamps(startTimestamp: any, endTimestamp: any) {
  if (typeof startTimestamp !== 'number' || typeof endTimestamp !== 'number') return null;
  if (!isFinite(startTimestamp) || !isFinite(endTimestamp) || endTimestamp < startTimestamp) return null;
  return Math.round((endTimestamp - startTimestamp) * 1000);
}

function publicNetEntry(entry: any, includeHeaders: any) {
  const output: any = {
    seq: entry.seq,
    ts: entry.ts,
    tab_id: tabIdString(entry.tabId),
    request_id: entry.request_id || '',
    method: entry.method || '',
    url: entry.url || '',
    status: typeof entry.status === 'number' ? entry.status : null,
    mime: typeof entry.mime === 'string' ? entry.mime : null,
    resource_type: entry.resource_type || 'other',
    from_cache: entry.from_cache === true,
    duration_ms: typeof entry.duration_ms === 'number' ? entry.duration_ms : null,
    error: typeof entry.error === 'string' ? entry.error : null,
    intercepted: entry.intercepted ? { action: entry.intercepted.action, rule_id: entry.intercepted.rule_id } : null,
  };
  if (includeHeaders) {
    output.request_headers = cloneHeaderMap(entry.request_headers);
    output.response_headers = cloneHeaderMap(entry.response_headers);
  }
  return output;
}

function appendNetEntry(capture: any, entry: any, capacity: any) {
  capture.entries.push(entry);
  if (entry.request_id) capture.byRequestId.set(entry.request_id, entry);
  while (capture.entries.length > capacity) {
    const removed = capture.entries.shift();
    if (removed && removed.request_id && capture.byRequestId.get(removed.request_id) === removed) {
      capture.byRequestId.delete(removed.request_id);
    }
  }
}

function filterNetEntries(entries: any, options: any) {
  const sinceSeq = options && Number.isInteger(options.sinceSeq) ? options.sinceSeq : 0;
  const limit = options && Number.isInteger(options.limit) ? options.limit : 100;
  const urlContains = options && typeof options.urlContains === 'string' ? options.urlContains.toLowerCase() : null;
  const method = options && typeof options.method === 'string' ? options.method.toUpperCase() : null;
  const resourceType = options && typeof options.resourceType === 'string' ? options.resourceType.toLowerCase() : null;
  const statusMin = options && Number.isInteger(options.statusMin) ? options.statusMin : null;
  const statusMax = options && Number.isInteger(options.statusMax) ? options.statusMax : null;
  const includeHeaders = !!(options && options.includeHeaders === true);
  const matches: any[] = [];

  (Array.isArray(entries) ? entries : []).forEach((entry) => {
    if (!entry || !Number.isInteger(entry.seq) || entry.seq <= sinceSeq) return;
    if (
      urlContains !== null &&
      String(entry.url || '')
        .toLowerCase()
        .indexOf(urlContains) === -1
    )
      return;
    if (method !== null && String(entry.method || '').toUpperCase() !== method) return;
    if (resourceType !== null && String(entry.resource_type || '').toLowerCase() !== resourceType) return;
    if (statusMin !== null || statusMax !== null) {
      if (typeof entry.status !== 'number') return;
      if (statusMin !== null && entry.status < statusMin) return;
      if (statusMax !== null && entry.status > statusMax) return;
    }
    matches.push(entry);
  });

  matches.sort((a, b) => a.seq - b.seq);
  const selected = matches.slice(0, limit);
  return {
    entries: selected.map((entry) => publicNetEntry(entry, includeHeaders)),
    next_seq: selected.length > 0 ? selected[selected.length - 1].seq : sinceSeq,
  };
}

function base64DecodedLength(text: any) {
  if (typeof text !== 'string') return -1;
  const clean = text.replace(/\s+/g, '');
  if (!/^[A-Za-z0-9+/]*={0,2}$/.test(clean)) return -1;
  if (clean.length % 4 === 1) return -1;
  const padding = clean.slice(-2) === '==' ? 2 : clean.slice(-1) === '=' ? 1 : 0;
  if (padding > 0 && clean.length % 4 !== 0) return -1;
  return Math.floor((clean.length * 3) / 4) - padding;
}

function validateInterceptRules(rawRules: any) {
  if (!Array.isArray(rawRules) || rawRules.length < 1 || rawRules.length > MAX_INTERCEPT_RULES) {
    throw new ToolError('BAD_ARGS', 'args.rules must contain between 1 and ' + MAX_INTERCEPT_RULES + ' rules', {
      count: Array.isArray(rawRules) ? rawRules.length : null,
    });
  }
  const seen = new Set();
  return rawRules.map((raw, index) => {
    if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
      throw new ToolError('BAD_ARGS', 'each intercept rule must be an object', { index: index });
    }
    const id = typeof raw.id === 'string' ? raw.id.trim() : '';
    if (!id || id.length > MAX_INTERCEPT_RULE_ID_LENGTH) {
      throw new ToolError(
        'BAD_ARGS',
        'rule id must be a non-empty string of at most ' + MAX_INTERCEPT_RULE_ID_LENGTH + ' characters',
        { index: index, rule_id: id || null },
      );
    }
    if (seen.has(id)) {
      throw new ToolError('BAD_ARGS', 'intercept rule ids must be unique', { rule_id: id });
    }
    seen.add(id);

    const action = typeof raw.action === 'string' ? raw.action.trim().toLowerCase() : '';
    if (action !== 'block' && action !== 'mock' && action !== 'continue') {
      throw new ToolError('BAD_ARGS', 'rule action must be block, mock or continue', { rule_id: id });
    }
    if (typeof raw.url_regex !== 'string' || raw.url_regex === '') {
      throw new ToolError('BAD_ARGS', 'rule url_regex must be a non-empty string', { rule_id: id });
    }
    let regex;
    try {
      regex = new RegExp(raw.url_regex);
    } catch (error) {
      throw new ToolError('BAD_ARGS', 'rule url_regex is not a valid regular expression: ' + describeError(error), {
        rule_id: id,
      });
    }

    let method = null;
    if (raw.method !== undefined && raw.method !== null) {
      if (typeof raw.method !== 'string' || raw.method.trim() === '') {
        throw new ToolError('BAD_ARGS', 'rule method must be a non-empty string', { rule_id: id });
      }
      method = raw.method.trim().toUpperCase();
    }
    let resourceType = null;
    if (raw.resource_type !== undefined && raw.resource_type !== null) {
      if (typeof raw.resource_type !== 'string' || raw.resource_type.trim() === '') {
        throw new ToolError('BAD_ARGS', 'rule resource_type must be a non-empty string', { rule_id: id });
      }
      resourceType = raw.resource_type.trim().toLowerCase();
    }

    let status = action === 'mock' ? 200 : null;
    let headers: any = null;
    let body = null;
    if (raw.status !== undefined && raw.status !== null) {
      if (action !== 'mock') {
        throw new ToolError('BAD_ARGS', 'rule status is only valid for mock rules', { rule_id: id });
      }
      status = readBoundedInteger(raw.status, 200, 100, 599, 'rules[' + index + '].status');
    }
    if (raw.headers !== undefined && raw.headers !== null) {
      if (action !== 'mock' || typeof raw.headers !== 'object' || Array.isArray(raw.headers)) {
        throw new ToolError('BAD_ARGS', 'rule headers must be a string map and are only valid for mock rules', {
          rule_id: id,
        });
      }
      headers = {};
      const names = Object.keys(raw.headers);
      for (let i = 0; i < names.length; i += 1) {
        if (typeof raw.headers[names[i]] !== 'string') {
          throw new ToolError('BAD_ARGS', 'rule header values must be strings', { rule_id: id, header: names[i] });
        }
        headers[names[i]] = raw.headers[names[i]];
      }
    }
    if (raw.body_base64 !== undefined && raw.body_base64 !== null) {
      if (action !== 'mock' || typeof raw.body_base64 !== 'string') {
        throw new ToolError('BAD_ARGS', 'rule body_base64 must be a base64 string and is only valid for mock rules', {
          rule_id: id,
        });
      }
      const decodedLength = base64DecodedLength(raw.body_base64);
      if (decodedLength < 0) {
        throw new ToolError('BAD_ARGS', 'rule body_base64 is not valid base64', { rule_id: id });
      }
      if (decodedLength > MAX_INTERCEPT_BODY_BYTES) {
        throw new ToolError('BAD_ARGS', 'rule body_base64 exceeds the decoded body limit', {
          rule_id: id,
          limit_bytes: MAX_INTERCEPT_BODY_BYTES,
        });
      }
      body = raw.body_base64;
    }

    const rule: any = {
      id: id,
      action: action,
      url_regex: raw.url_regex,
      method: method,
      resource_type: resourceType,
      status: status,
      headers: headers,
      body_base64: body,
    };
    Object.defineProperty(rule, 'regex', { value: regex, enumerable: false, writable: false, configurable: false });
    return rule;
  });
}

function publicInterceptRule(rule: any) {
  const output: any = { id: rule.id, action: rule.action, url_regex: rule.url_regex };
  if (rule.method) output.method = rule.method;
  if (rule.resource_type) output.resource_type = rule.resource_type;
  return output;
}

function matchInterceptRule(rules: any, request: any, resourceType: any) {
  const list = Array.isArray(rules) ? rules : [];
  for (let i = 0; i < list.length; i += 1) {
    const rule = list[i];
    if (!rule || !rule.regex) continue;
    if (rule.method && String(request && request.method ? request.method : '').toUpperCase() !== rule.method) continue;
    if (rule.resource_type && String(resourceType || '').toLowerCase() !== rule.resource_type) continue;
    rule.regex.lastIndex = 0;
    if (rule.regex.test(String(request && request.url ? request.url : ''))) return rule;
  }
  return null;
}

function buildInterceptFulfillParams(requestId: any, rule: any) {
  const headers: any = rule.headers ? cloneHeaderMap(rule.headers) : {};
  const names = Object.keys(headers);
  if (rule.body_base64 !== null && rule.body_base64 !== undefined) {
    let hasContentType = false;
    for (let i = 0; i < names.length; i += 1) {
      if (names[i].toLowerCase() === 'content-type') {
        hasContentType = true;
        break;
      }
    }
    if (!hasContentType) headers['content-type'] = 'text/plain; charset=utf-8';
  }
  const responseHeaders = Object.keys(headers).map((name) => ({ name: name, value: String(headers[name]) }));
  const params: any = {
    requestId: requestId,
    responseCode: rule.status || 200,
    responseHeaders: responseHeaders,
  };
  if (rule.body_base64 !== null && rule.body_base64 !== undefined) params.body = rule.body_base64;
  return params;
}

function rulesForTab(tabId: any) {
  const numeric = typeof tabId === 'number' ? tabId : Number(tabId);
  const tabRules = tabInterceptRules.get(numeric) || [];
  return tabRules.concat(browserInterceptRules);
}

function captureForTab(tabId: any, create: any) {
  const numeric = typeof tabId === 'number' ? tabId : Number(tabId);
  let capture = networkCaptures.get(numeric);
  if (!capture && create) {
    capture = {
      tabId: numeric,
      capturing: false,
      startedAt: null,
      entries: [],
      byRequestId: new Map(),
      pendingIntercepts: new Map(),
      fetchEnabled: false,
      lastInterceptError: null,
    };
    networkCaptures.set(numeric, capture);
  }
  return capture || null;
}

function markNetworkCaptureDetached(tabId: any) {
  const capture = captureForTab(tabId, false);
  if (!capture) return;
  capture.capturing = false;
  capture.fetchEnabled = false;
}

function cleanupNetworkForTab(tabId: any) {
  const numeric = typeof tabId === 'number' ? tabId : Number(tabId);
  networkCaptures.delete(numeric);
  tabInterceptRules.delete(numeric);
}

// ---------------------------------------------------------------------------
// CDP Network/Fetch event handling
// ---------------------------------------------------------------------------

function requestEntryForEvent(capture: any, params: any) {
  const request = params && params.request && typeof params.request === 'object' ? params.request : {};
  const wallTime = params && typeof params.wallTime === 'number' ? Math.round(params.wallTime * 1000) : Date.now();
  return {
    seq: networkSeq + 1,
    ts: wallTime,
    tabId: capture.tabId,
    request_id: String((params && params.requestId) || ''),
    method: request.method || 'GET',
    url: request.url || '',
    status: null,
    mime: null,
    resource_type: (params && params.type) || 'other',
    from_cache: false,
    duration_ms: null,
    error: null,
    intercepted: null,
    request_headers: cloneHeaderMap(request.headers),
    response_headers: null,
    started_timestamp: params && typeof params.timestamp === 'number' ? params.timestamp : null,
  };
}

function applyResponseToEntry(entry: any, response: any) {
  const body = response && typeof response === 'object' ? response : {};
  entry.status = typeof body.status === 'number' ? body.status : null;
  entry.mime = typeof body.mimeType === 'string' ? body.mimeType : null;
  entry.from_cache =
    body.fromDiskCache === true ||
    body.fromServiceWorker === true ||
    body.fromPrefetchCache === true ||
    body.fromMemoryCache === true;
  entry.response_headers = cloneHeaderMap(body.headers);
}

function applyPendingIntercept(capture: any, entry: any) {
  if (!entry || !entry.request_id) return;
  const pending = capture.pendingIntercepts.get(entry.request_id);
  if (!pending) return;
  entry.intercepted = pending;
  capture.pendingIntercepts.delete(entry.request_id);
}

function handleNetworkEvent(tabId: any, method: any, params: any) {
  const capture = captureForTab(tabId, false);
  if (!capture || !capture.capturing) return;
  const body = params && typeof params === 'object' ? params : {};

  if (method === 'Network.requestWillBeSent') {
    const requestId = String(body.requestId || '');
    const previous = capture.byRequestId.get(requestId);
    if (previous && body.redirectResponse) {
      applyResponseToEntry(previous, body.redirectResponse);
      if (previous.started_timestamp !== null && typeof body.timestamp === 'number') {
        previous.duration_ms = durationFromTimestamps(previous.started_timestamp, body.timestamp);
      }
    }
    networkSeq += 1;
    const entry = requestEntryForEvent(capture, body);
    entry.seq = networkSeq;
    applyPendingIntercept(capture, entry);
    appendNetEntry(capture, entry, NET_CAPTURE_CAPACITY);
    return;
  }

  if (method === 'Network.responseReceived') {
    const entry = capture.byRequestId.get(String(body.requestId || ''));
    if (!entry) return;
    applyResponseToEntry(entry, body.response);
    return;
  }

  if (method === 'Network.loadingFinished') {
    const entry = capture.byRequestId.get(String(body.requestId || ''));
    if (!entry) return;
    if (entry.started_timestamp !== null && typeof body.timestamp === 'number') {
      entry.duration_ms = durationFromTimestamps(entry.started_timestamp, body.timestamp);
    }
    return;
  }

  if (method === 'Network.loadingFailed') {
    const entry = capture.byRequestId.get(String(body.requestId || ''));
    if (!entry) return;
    entry.error = String(body.errorText || body.blockedReason || 'failed');
    if (entry.started_timestamp !== null && typeof body.timestamp === 'number') {
      entry.duration_ms = durationFromTimestamps(entry.started_timestamp, body.timestamp);
    }
  }
}

function applyInterceptTag(capture: any, requestId: any, rule: any) {
  if (!capture || !requestId || !rule) return;
  const tag = { action: rule.action, rule_id: rule.id };
  const entry = capture.byRequestId.get(requestId);
  if (entry) {
    entry.intercepted = tag;
    return;
  }
  capture.pendingIntercepts.set(requestId, tag);
  if (capture.pendingIntercepts.size > 500) {
    const first = capture.pendingIntercepts.keys().next();
    if (!first.done) capture.pendingIntercepts.delete(first.value);
  }
}

async function ensureFetchForTab(tabId: any) {
  const capture = captureForTab(tabId, false);
  if (!capture || capture.fetchEnabled) return;
  await sendCdpCommand(tabId, 'Fetch.enable', { patterns: [{ urlPattern: '*', requestStage: 'Request' }] });
  capture.fetchEnabled = true;
}

async function disableFetchForTab(tabId: any) {
  const capture = captureForTab(tabId, false);
  if (!capture || !capture.fetchEnabled) return;
  capture.fetchEnabled = false;
  try {
    await sendCdpCommand(tabId, 'Fetch.disable', {});
  } catch (error) {
    log('Fetch.disable failed', describeError(error));
  }
}

async function syncFetchForAllCaptures() {
  const ids = Array.from(networkCaptures.keys());
  for (let i = 0; i < ids.length; i += 1) {
    const capture = networkCaptures.get(ids[i]);
    if (!capture || !capture.capturing) continue;
    if (rulesForTab(ids[i]).length > 0) await ensureFetchForTab(ids[i]);
    else await disableFetchForTab(ids[i]);
  }
}

async function handleFetchRequestPaused(tabId: any, params: any) {
  const body = params && typeof params === 'object' ? params : {};
  const requestId = String(body.requestId || '');
  if (!requestId) return;
  const request = body.request && typeof body.request === 'object' ? body.request : {};
  const capture = captureForTab(tabId, false);
  const rule = matchInterceptRule(rulesForTab(tabId), request, body.resourceType);
  if (capture) applyInterceptTag(capture, String(body.networkId || requestId), rule);

  if (!rule) {
    await sendCdpCommand(tabId, 'Fetch.continueRequest', { requestId: requestId });
    return;
  }

  try {
    if (rule.action === 'block') {
      await sendCdpCommand(tabId, 'Fetch.failRequest', { requestId: requestId, errorReason: 'BlockedByClient' });
    } else if (rule.action === 'mock') {
      await sendCdpCommand(tabId, 'Fetch.fulfillRequest', buildInterceptFulfillParams(requestId, rule));
    } else {
      await sendCdpCommand(tabId, 'Fetch.continueRequest', { requestId: requestId });
    }
  } catch (error) {
    if (capture) capture.lastInterceptError = { rule_id: rule.id, action: rule.action, message: describeError(error) };
    log('intercept action failed', rule.id, describeError(error));
    try {
      await sendCdpCommand(tabId, 'Fetch.continueRequest', { requestId: requestId });
    } catch (fallbackError) {
      log('intercept continue fallback failed', describeError(fallbackError));
    }
  }
}

function handleDebuggerEvent(source: any, method: any, params: any) {
  const tabId = source && typeof source.tabId === 'number' ? source.tabId : null;
  if (tabId === null || typeof method !== 'string') return;
  if (method.indexOf('Network.') === 0) {
    handleNetworkEvent(tabId, method, params);
    touchDebuggerChannel(tabId);
    return;
  }
  if (method === 'Fetch.requestPaused') {
    touchDebuggerChannel(tabId);
    void handleFetchRequestPaused(tabId, params).catch((error) => {
      log('Fetch.requestPaused failed', describeError(error));
    });
  }
}

function handleDebuggerDetach(tabId: any, reason: any) {
  markNetworkCaptureDetached(tabId);
  clearDebuggerSession(tabId);
  log('debugger detached by target', tabId, reason || 'unknown reason');
}

// ---------------------------------------------------------------------------
// Network tools
// ---------------------------------------------------------------------------

async function startNetworkCapture(tab: any) {
  const capture = captureForTab(tab.id, true);
  if (capture.capturing) return capture;
  capture.entries = [];
  capture.byRequestId = new Map();
  capture.pendingIntercepts = new Map();
  capture.fetchEnabled = false;
  capture.lastInterceptError = null;
  capture.startedAt = Date.now();
  capture.capturing = true;
  try {
    await sendCdpCommand(tab.id, 'Network.enable', {});
    if (rulesForTab(tab.id).length > 0) await ensureFetchForTab(tab.id);
    return capture;
  } catch (error: any) {
    capture.capturing = false;
    capture.startedAt = null;
    capture.fetchEnabled = false;
    if (!error || error.code !== 'CAPABILITY_UNAVAILABLE') {
      try {
        await sendCdpCommand(tab.id, 'Network.disable', {});
      } catch (disableError) {
        log('Network.disable rollback failed', describeError(disableError));
      }
    }
    throw error;
  }
}

async function stopNetworkCapture(tab: any) {
  const capture = captureForTab(tab.id, false);
  if (!capture) return null;
  capture.capturing = false;
  capture.pendingIntercepts.clear();
  if (debuggerSessions.has(tab.id)) {
    if (capture.fetchEnabled) await disableFetchForTab(tab.id);
    try {
      await sendCdpCommand(tab.id, 'Network.disable', {});
    } catch (error) {
      log('Network.disable failed', describeError(error));
    }
  }
  capture.fetchEnabled = false;
  return capture;
}

async function netCaptureTool(args: any) {
  const rawAction = args ? args.action : undefined;
  if (typeof rawAction !== 'string' || rawAction.trim() === '') {
    throw new ToolError('BAD_ARGS', 'args.action must be start, stop or status', {
      action: rawAction === undefined ? null : rawAction,
    });
  }
  const action = rawAction.trim().toLowerCase();
  if (action !== 'start' && action !== 'stop' && action !== 'status') {
    throw new ToolError('BAD_ARGS', 'args.action must be start, stop or status', { action: rawAction });
  }
  const tab = await resolveTab(args);
  if (!tab || typeof tab.id !== 'number') throw new ToolError('NOT_FOUND', 'no active tab is available');

  if (action === 'start') {
    const capture = await startNetworkCapture(tab);
    return {
      capturing: capture.capturing === true,
      tab_id: tabIdString(tab.id),
      started_at: capture.startedAt,
      capacity: NET_CAPTURE_CAPACITY,
    };
  }

  if (action === 'stop') {
    const capture = await stopNetworkCapture(tab);
    return {
      capturing: false,
      tab_id: tabIdString(tab.id),
      entries: capture ? capture.entries.length : 0,
    };
  }

  const capture = captureForTab(tab.id, false);
  const result: any = {
    capturing: !!(capture && capture.capturing),
    tab_id: tabIdString(tab.id),
    entries: capture ? capture.entries.length : 0,
  };
  if (capture && typeof capture.startedAt === 'number') result.started_at = capture.startedAt;
  return result;
}

async function netLogTool(args: any) {
  const sinceSeq = readBoundedInteger(args.since_seq, 0, 0, Number.MAX_SAFE_INTEGER, 'args.since_seq');
  const limit = readBoundedInteger(args.limit, 100, 1, 1000, 'args.limit');
  const includeHeaders = readBoolean(args.include_headers, false, 'args.include_headers');
  const urlContains = readOptionalString(args.url_contains, 'args.url_contains');
  const method = readOptionalString(args.method, 'args.method');
  const resourceType = readOptionalString(args.resource_type, 'args.resource_type');
  const statusMin =
    args.status_min === undefined || args.status_min === null
      ? null
      : readBoundedInteger(args.status_min, 0, 0, 999, 'args.status_min');
  const statusMax =
    args.status_max === undefined || args.status_max === null
      ? null
      : readBoundedInteger(args.status_max, 999, 0, 999, 'args.status_max');
  if (statusMin !== null && statusMax !== null && statusMin > statusMax) {
    throw new ToolError('BAD_ARGS', 'args.status_min must not exceed args.status_max', {
      status_min: statusMin,
      status_max: statusMax,
    });
  }

  let captures: any[] = [];
  if (hasTabIdArgument(args)) {
    const tab = await resolveTab(args);
    const capture = captureForTab(tab!.id, false);
    if (capture) captures.push(capture);
  } else {
    captures = Array.from(networkCaptures.values());
  }

  const allEntries: any[] = [];
  let capturing = false;
  captures.forEach((capture) => {
    if (capture && capture.capturing) capturing = true;
    if (!capture) return;
    for (let i = 0; i < capture.entries.length; i += 1) allEntries.push(capture.entries[i]);
  });

  const filtered = filterNetEntries(allEntries, {
    sinceSeq: sinceSeq,
    limit: limit,
    urlContains: urlContains,
    method: method,
    resourceType: resourceType,
    statusMin: statusMin,
    statusMax: statusMax,
    includeHeaders: includeHeaders,
  });
  return { entries: filtered.entries, next_seq: filtered.next_seq, capturing: capturing };
}

async function netClearTool(args: any) {
  let captures = [];
  if (hasTabIdArgument(args)) {
    const tab = await resolveTab(args);
    const capture = captureForTab(tab!.id, false);
    if (capture) captures.push(capture);
  } else {
    captures = Array.from(networkCaptures.values());
  }
  let cleared = 0;
  captures.forEach((capture) => {
    if (!capture) return;
    cleared += capture.entries.length;
    capture.entries = [];
    capture.byRequestId = new Map();
    capture.pendingIntercepts.clear();
  });
  return { cleared: cleared };
}

async function netInterceptTool(args: any) {
  const rawAction = args ? args.action : undefined;
  if (typeof rawAction !== 'string' || rawAction.trim() === '') {
    throw new ToolError('BAD_ARGS', 'args.action must be set or clear', {
      action: rawAction === undefined ? null : rawAction,
    });
  }
  const action = rawAction.trim().toLowerCase();
  if (action !== 'set' && action !== 'clear') {
    throw new ToolError('BAD_ARGS', 'args.action must be set or clear', { action: rawAction });
  }
  const tabScoped = hasTabIdArgument(args);
  let tab: any = null;
  if (tabScoped) {
    tab = await resolveTab(args);
    if (!tab || typeof tab.id !== 'number') throw new ToolError('NOT_FOUND', 'the target tab does not exist');
  }

  if (action === 'clear') {
    if (tabScoped) {
      tabInterceptRules.delete(tab.id);
      const capture = captureForTab(tab.id, false);
      if (capture && capture.capturing) {
        if (rulesForTab(tab.id).length > 0) await ensureFetchForTab(tab.id);
        else await disableFetchForTab(tab.id);
      }
      return { ok: true, scope: 'tab', tab_id: tabIdString(tab.id), rules: [], applied: true };
    }
    browserInterceptRules = [];
    await syncFetchForAllCaptures();
    return { ok: true, scope: 'browser', rules: [], applied: true };
  }

  const rules = validateInterceptRules(args.rules);
  if (tabScoped) {
    await startNetworkCapture(tab);
    const previous = tabInterceptRules.get(tab.id) || null;
    tabInterceptRules.set(tab.id, rules);
    try {
      await ensureFetchForTab(tab.id);
    } catch (error) {
      if (previous === null) tabInterceptRules.delete(tab.id);
      else tabInterceptRules.set(tab.id, previous);
      throw error;
    }
    return {
      ok: true,
      scope: 'tab',
      tab_id: tabIdString(tab.id),
      rules: rules.map(publicInterceptRule),
      applied: true,
    };
  }

  const previous = browserInterceptRules;
  browserInterceptRules = rules;
  try {
    await syncFetchForAllCaptures();
  } catch (error) {
    browserInterceptRules = previous;
    try {
      await syncFetchForAllCaptures();
    } catch (rollbackError) {
      log('intercept rollback sync failed', describeError(rollbackError));
    }
    throw error;
  }
  return { ok: true, scope: 'browser', rules: rules.map(publicInterceptRule), applied: true };
}

// ---------------------------------------------------------------------------
// Downloads
// ---------------------------------------------------------------------------

function downloadSummary(item: any, includeTab: any) {
  const started = item && item.startTime ? Date.parse(item.startTime) : NaN;
  const result: any = {
    id: item && typeof item.id === 'number' ? item.id : null,
    filename: item && typeof item.filename === 'string' ? item.filename : '',
    url: item && typeof item.url === 'string' ? item.url : '',
    state: item && typeof item.state === 'string' ? item.state : '',
    bytes_received: item && typeof item.bytesReceived === 'number' ? item.bytesReceived : 0,
    total_bytes: item && typeof item.totalBytes === 'number' ? item.totalBytes : 0,
    danger: item && typeof item.danger === 'string' ? item.danger : 'safe',
    exists: !!(item && item.exists === true),
    started_at: Number.isFinite(started) ? started : null,
  };
  if (includeTab && item && typeof item.tabId === 'number' && item.tabId >= 0) {
    result.tab_id = tabIdString(item.tabId);
  }
  return result;
}

function downloadSortValue(item: any) {
  const started = item && item.startTime ? Date.parse(item.startTime) : NaN;
  return Number.isFinite(started) ? started : 0;
}

// A download that started just before download_wait was called must still be a
// candidate. Prefer the newest such download and ignore anything that started
// more than five seconds before the call, so an unrelated old entry never wins.
function selectDownloadWaitCandidate(items: any, baselineIds: any, callStartedAt: any, recentWindowMs: any) {
  const cutoff = callStartedAt - recentWindowMs;
  const candidates = [];
  for (let i = 0; i < items.length; i += 1) {
    const item = items[i];
    if (!item || typeof item.id !== 'number') continue;
    const started = downloadSortValue(item);
    const isNew = !baselineIds.has(item.id);
    const isRecent = started > 0 && started >= cutoff;
    if (!isNew && !isRecent) continue;
    if (started > 0 && started < cutoff) continue;
    candidates.push(item);
  }
  candidates.sort((a, b) => {
    const delta = downloadSortValue(b) - downloadSortValue(a);
    if (delta !== 0) return delta;
    return b.id - a.id;
  });
  return candidates.length > 0 ? candidates[0] : null;
}

async function downloadListTool(args: any) {
  const limit = readBoundedInteger(args.limit, 50, 1, 200, 'args.limit');
  let items;
  try {
    items = await chrome.downloads.search({});
  } catch (error) {
    throw new ToolError('INTERNAL', 'the downloads list could not be read: ' + describeError(error));
  }
  const sorted = (Array.isArray(items) ? items.slice() : []).sort((a, b) => {
    const delta = downloadSortValue(b) - downloadSortValue(a);
    if (delta !== 0) return delta;
    return (b && typeof b.id === 'number' ? b.id : 0) - (a && typeof a.id === 'number' ? a.id : 0);
  });
  return { downloads: sorted.slice(0, limit).map((item) => downloadSummary(item, true)) };
}

async function downloadWaitTool(args: any, frame: any, signal: any) {
  const id = parseDownloadId(args.id);
  const timeoutMs = readBoundedInteger(args.timeout_ms, 30000, 100, 120000, 'args.timeout_ms');
  const startedAt = Date.now();
  throwIfCancelled(signal);
  let baselineIds = null;
  if (id === null) {
    let items;
    try {
      items = await chrome.downloads.search({});
    } catch (error) {
      throw new ToolError('INTERNAL', 'the downloads list could not be read: ' + describeError(error));
    }
    baselineIds = new Set(
      (Array.isArray(items) ? items : []).filter((item) => item && typeof item.id === 'number').map((item) => item.id),
    );
  }

  while (true) {
    throwIfCancelled(signal);
    let items;
    try {
      items = await chrome.downloads.search(id === null ? {} : { id: id });
    } catch (error) {
      throw new ToolError('INTERNAL', 'the download state could not be read: ' + describeError(error));
    }
    const list = Array.isArray(items) ? items : [];
    let item = null;
    if (id !== null) {
      for (let i = 0; i < list.length; i += 1) {
        if (list[i] && list[i].id === id) {
          item = list[i];
          break;
        }
      }
    } else {
      item = selectDownloadWaitCandidate(list, baselineIds, startedAt, DOWNLOAD_RECENT_WINDOW_MS);
    }

    if (item && (item.state === 'complete' || item.state === 'interrupted')) {
      const summary = downloadSummary(item, false);
      return {
        id: summary.id,
        state: summary.state,
        filename: summary.filename,
        url: summary.url,
        bytes_received: summary.bytes_received,
        total_bytes: summary.total_bytes,
      };
    }

    const waited = Date.now() - startedAt;
    if (waited >= timeoutMs) {
      const details: any = { waited_ms: waited };
      if (id !== null) details.id = id;
      throw new ToolError('TIMEOUT', 'no download finished within ' + timeoutMs + ' ms', details);
    }
    await sleep(DOWNLOAD_POLL_MS);
    throwIfCancelled(signal);
  }
}

// ---------------------------------------------------------------------------
// File uploads
// ---------------------------------------------------------------------------

function filePathToFileUrl(filePath: any) {
  const normalized = String(filePath).replace(/\\/g, '/');
  if (/^[A-Za-z]:\//.test(normalized)) return 'file:///' + normalized;
  if (normalized.indexOf('//') === 0) return 'file:' + normalized;
  if (normalized.charAt(0) === '/') return 'file://' + normalized;
  return 'file:///' + normalized;
}

// DOM.setFileInputFiles can attach a path that does not exist, producing an
// empty File entry instead of a CDP error. The extension page can read file
// URLs under its host permission, so probe each path once before attaching it.
async function ensureUploadFilesExist(files: any) {
  const results = await Promise.all(
    files.map(async (file: any) => {
      try {
        const response = await fetch(filePathToFileUrl(file), { method: 'HEAD', cache: 'no-store' });
        if (response && response.ok) return { file: file, ok: true };
        return { file: file, ok: false };
      } catch (error) {
        return { file: file, ok: false };
      }
    }),
  );
  for (let i = 0; i < results.length; i += 1) {
    if (!results[i].ok) {
      throw new ToolError('BAD_ARGS', 'the file could not be read: ' + results[i].file, { path: results[i].file });
    }
  }
}

// chrome.debugger.sendCommand resolves the CDP result object. DOM.getDocument
// wraps the root as {root:{nodeId}}, while DOM.querySelector returns {nodeId}
// directly, so read both shapes instead of assuming the root is top-level.
function readCdpNodeId(result: any) {
  if (!result || typeof result !== 'object') return null;
  if (typeof result.nodeId === 'number' && result.nodeId > 0) return result.nodeId;
  if (result.root && typeof result.root.nodeId === 'number' && result.root.nodeId > 0) {
    return result.root.nodeId;
  }
  return null;
}

async function uploadFileTool(args: any) {
  const target = args && typeof args.target === 'string' ? args.target.trim() : '';
  if (!target) {
    throw new ToolError('BAD_ARGS', 'args.target is required; use an @eN reference or a CSS selector');
  }
  if (!Array.isArray(args.files) || args.files.length < 1 || args.files.length > MAX_UPLOAD_FILES) {
    throw new ToolError('BAD_ARGS', 'args.files must contain between 1 and ' + MAX_UPLOAD_FILES + ' absolute paths', {
      count: Array.isArray(args.files) ? args.files.length : null,
    });
  }
  const files = [];
  for (let i = 0; i < args.files.length; i += 1) {
    const raw = args.files[i];
    if (!isAbsoluteFilePath(raw)) {
      throw new ToolError('BAD_ARGS', 'each upload path must be an absolute file path', {
        path: raw === undefined || raw === null ? null : String(raw),
      });
    }
    files.push(raw.trim());
  }
  await ensureUploadFilesExist(files);

  const tab = await resolveTab(args);
  if (!tab || typeof tab.id !== 'number') throw new ToolError('NOT_FOUND', 'no active tab is available');
  if (isRestrictedUrl(tab.url)) {
    throw new ToolError('CAPABILITY_UNAVAILABLE', 'this page cannot be scripted: ' + String(tab.url), {
      url: tab.url,
      reason: 'restricted_scheme',
    });
  }

  const uploadTarget = await requestContentResult(tab, { type: 'corvus.upload_target', target: target });
  if (!uploadTarget || typeof uploadTarget.selector !== 'string' || uploadTarget.selector === '') {
    throw new ToolError('INTERNAL', 'the file input could not be resolved to a selector');
  }

  const documentResult = await sendCdpCommand(tab.id, 'DOM.getDocument', { depth: 0 });
  const rootNodeId = readCdpNodeId(documentResult);
  if (rootNodeId === null) throw new ToolError('INTERNAL', 'the document root could not be read');

  let found;
  try {
    found = await sendCdpCommand(tab.id, 'DOM.querySelector', { nodeId: rootNodeId, selector: uploadTarget.selector });
  } catch (error: any) {
    if (error && error.code === 'NOT_FOUND') throw error;
    throw new ToolError('NOT_FOUND', 'the file input disappeared from the page', { selector: uploadTarget.selector });
  }
  const foundNodeId = readCdpNodeId(found);
  if (foundNodeId === null) {
    throw new ToolError('NOT_FOUND', 'the file input disappeared from the page', { selector: uploadTarget.selector });
  }

  const described: any = await sendCdpCommand(tab.id, 'DOM.describeNode', { nodeId: foundNodeId, depth: 0 });
  const node = described && described.node ? described.node : null;
  const nodeName = node && typeof node.nodeName === 'string' ? node.nodeName.toUpperCase() : '';
  const attributes = node && Array.isArray(node.attributes) ? node.attributes : [];
  let inputType = '';
  for (let i = 0; i + 1 < attributes.length; i += 2) {
    if (String(attributes[i]).toLowerCase() === 'type') {
      inputType = String(attributes[i + 1]).toLowerCase();
      break;
    }
  }
  if (nodeName !== 'INPUT' || inputType !== 'file') {
    throw new ToolError('BAD_ARGS', 'the target is not a file input', {
      reason: 'not_a_file_input',
      tag: nodeName.toLowerCase(),
      input_type: inputType || null,
    });
  }

  try {
    await sendCdpCommand(tab.id, 'DOM.setFileInputFiles', { files: files, nodeId: foundNodeId });
  } catch (error: any) {
    if (error && error.code === 'CAPABILITY_UNAVAILABLE') throw error;
    const message = describeError(error);
    if (/No such file|not found|Could not read file|failed to read|ENOENT|unable to read/i.test(message)) {
      let missingPath = files[0];
      for (let i = 0; i < files.length; i += 1) {
        if (message.indexOf(files[i]) !== -1) {
          missingPath = files[i];
          break;
        }
      }
      throw new ToolError('BAD_ARGS', 'the file could not be attached: ' + message, { path: missingPath });
    }
    throw new ToolError('INTERNAL', 'the file selection failed: ' + message);
  }

  const selectorLiteral = JSON.stringify(uploadTarget.selector);
  const script =
    '(function () {' +
    ' var el = document.querySelector(' +
    selectorLiteral +
    ');' +
    ' if (!el) return false;' +
    ' el.dispatchEvent(new Event("input", { bubbles: true, composed: true }));' +
    ' el.dispatchEvent(new Event("change", { bubbles: true, composed: true }));' +
    ' return true;' +
    '})()';
  const dispatchResult: any = await sendCdpCommand(tab.id, 'Runtime.evaluate', {
    expression: script,
    returnByValue: true,
    userGesture: true,
  });
  if (dispatchResult && dispatchResult.exceptionDetails) {
    throw new ToolError('INTERNAL', 'the input and change events could not be dispatched');
  }
  if (dispatchResult && dispatchResult.result && dispatchResult.result.value === false) {
    throw new ToolError('NOT_FOUND', 'the file input disappeared after files were attached', {
      selector: uploadTarget.selector,
    });
  }

  const resolvedTarget: any = { tag: uploadTarget.tag || 'input', text: uploadTarget.text || '' };
  if (uploadTarget.ref) resolvedTarget.ref = uploadTarget.ref;
  const input: any = { multiple: uploadTarget.multiple === true };
  if (uploadTarget.accept) input.accept = uploadTarget.accept;
  return { uploaded: true, files: files, target: resolvedTarget, input: input };
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Navigation: issue, wait for the new document, read the final URL
// ---------------------------------------------------------------------------

const navigationEpochs = new Map();

function beginNavigationEpoch(tabId: any) {
  const next = (navigationEpochs.get(tabId) || 0) + 1;
  navigationEpochs.set(tabId, next);
  return next;
}

function isErrorPageUrl(url: any) {
  return typeof url === 'string' && /^(chrome|edge)-error:\/\//i.test(url.trim());
}

function isUnscriptablePageUrl(url: any) {
  if (typeof url !== 'string') return true;
  const trimmed = url.trim();
  if (!trimmed || trimmed.toLowerCase() === 'about:blank') return true;
  return isRestrictedUrl(trimmed) || isErrorPageUrl(trimmed);
}

function isNavigableUrl(url: any) {
  return typeof url === 'string' && /^(https?|file):/i.test(url.trim());
}

function sameDocumentExceptHash(left: any, right: any) {
  try {
    const a = new URL(left);
    const b = new URL(right);
    return a.origin === b.origin && a.pathname === b.pathname && a.search === b.search;
  } catch (error) {
    return false;
  }
}

function urlMatchesTarget(url: any, target: any) {
  if (!target) return true;
  try {
    const a = new URL(String(url));
    const b = new URL(String(target));
    a.hash = '';
    b.hash = '';
    return a.href === b.href;
  } catch (error) {
    return String(url) === String(target);
  }
}

async function updateTabUrl(tabId: any, url: any) {
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

function waitForDocumentLoad(tabId: any, beforeUrl: any, targetUrl: any, epoch: any, timeoutMs: any) {
  return new Promise((resolve) => {
    let settled = false;
    let sawLoading = false;
    let timer: ReturnType<typeof setTimeout> | null = null;

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

    function finish(loaded: any, url: any) {
      if (settled) return;
      settled = true;
      cleanup();
      resolve({ loaded: loaded === true, url: url || null });
    }

    function onUpdated(updatedTabId: any, changeInfo: any, tab: any) {
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

    timer = setTimeout(
      async () => {
        if (settled) return;
        let url = null;
        try {
          const tab = await chrome.tabs.get(tabId);
          url = tab && tab.url ? tab.url : null;
        } catch (error) {
          url = null;
        }
        finish(false, url);
      },
      Math.max(1, timeoutMs),
    );

    // The load may already be complete before the listener was attached.
    chrome.tabs
      .get(tabId)
      .then((tab) => {
        if (settled || !tab) return;
        if ((navigationEpochs.get(tabId) || 0) !== epoch) {
          finish(false, tab.url || null);
          return;
        }
        const url = tab.url || null;
        if (tab.status === 'complete' && url && (!beforeUrl || url !== beforeUrl)) finish(true, url);
      })
      .catch(() => {
        // The timeout path reports the final state.
      });
  });
}

function waitForNewTabLoad(tabId: any, targetUrl: any, epoch: any, timeoutMs: any) {
  return new Promise((resolve) => {
    let settled = false;
    let sawLoading = false;
    let timer: ReturnType<typeof setTimeout> | null = null;

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

    function finish(loaded: any, url: any) {
      if (settled) return;
      settled = true;
      cleanup();
      resolve({ loaded: loaded === true, url: url || null });
    }

    function targetReached(url: any) {
      return targetUrl ? urlMatchesTarget(url, targetUrl) : true;
    }

    function onUpdated(updatedTabId: any, changeInfo: any, tab: any) {
      if (settled || updatedTabId !== tabId || !changeInfo) return;
      if ((navigationEpochs.get(tabId) || 0) !== epoch) {
        finish(false, tab && tab.url ? tab.url : null);
        return;
      }
      if (changeInfo.status === 'loading') {
        sawLoading = true;
        return;
      }
      if (changeInfo.status !== 'complete') return;
      const url = (tab && tab.url) || changeInfo.url || null;
      // A brand new tab can report a complete about:blank before the requested
      // document starts loading. Only accept that event if it is the target.
      if (!sawLoading && !targetReached(url)) return;
      finish(true, url);
    }

    chrome.tabs.onUpdated.addListener(onUpdated);

    timer = setTimeout(
      async () => {
        if (settled) return;
        let url = null;
        try {
          const tab = await chrome.tabs.get(tabId);
          url = tab && tab.url ? tab.url : null;
        } catch (error) {
          url = null;
        }
        finish(false, url);
      },
      Math.max(1, timeoutMs),
    );

    chrome.tabs
      .get(tabId)
      .then((tab) => {
        if (settled || !tab) return;
        if ((navigationEpochs.get(tabId) || 0) !== epoch) {
          finish(false, tab.url || null);
          return;
        }
        const url = tab.url || null;
        if (tab.status === 'complete' && targetReached(url)) finish(true, url);
      })
      .catch(() => {
        // The timeout path reports the final state.
      });
  });
}

async function navigateTool(args: any, frame: any, signal: any) {
  const tab = await resolveTab(args);
  if (!tab || typeof tab.id !== 'number') {
    throw new ToolError('NOT_FOUND', 'no active tab is available');
  }

  const raw = args && typeof args.url === 'string' ? args.url : args && typeof args.href === 'string' ? args.href : '';
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
    throw new ToolError('BAD_ARGS', 'only http, https and file URLs can be navigated to', {
      url: resolved.href.slice(0, 200),
    });
  }
  const target = resolved.href;

  let stopRequested = false;
  const stopLoading = () => {
    if (stopRequested) return;
    stopRequested = true;
    void sendCdpCommand(tab.id, 'Page.stopLoading', {}).catch((error) => {
      log('Page.stopLoading failed', describeError(error));
    });
  };
  if (signal) {
    if (signal.aborted) {
      stopLoading();
      throw cancelledError();
    }
    signal.addEventListener('abort', stopLoading, { once: true });
  }

  const timeoutMs = clampTimeout(frame && frame.meta ? frame.meta.timeout_ms : undefined);
  // Keep at least 2 s of the tool budget for the reply and stay below 15 s.
  const waitBudget = Math.max(0, Math.min(timeoutMs - 2000, 15000));
  const epoch = beginNavigationEpoch(tab.id);

  let started = null;
  if (isUnscriptablePageUrl(beforeUrl)) {
    // A restricted new-tab or error page cannot host a content script; navigate it directly.
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
  throwIfCancelled(signal);

  const hashOnly = beforeUrl !== null && target !== beforeUrl && sameDocumentExceptHash(beforeUrl, target);
  let loaded = hashOnly === true;
  let finalUrl = target;

  if (!hashOnly && waitBudget > 0) {
    const outcome = await raceWithAbort(waitForDocumentLoad(tab.id, beforeUrl, target, epoch, waitBudget), signal);
    loaded = outcome.loaded;
    finalUrl = outcome.url || target;
  }

  throwIfCancelled(signal);

  let current = null;
  try {
    current = await chrome.tabs.get(tab.id);
  } catch (error) {
    throw new ToolError('BROWSER_GONE', 'the tab was closed while navigation was in progress', {
      reason: 'tab_closed',
    });
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

  throwIfCancelled(signal);
  if (signal) signal.removeEventListener('abort', stopLoading);
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
// Debugger channel (managed automatically, never by the user)
// ---------------------------------------------------------------------------
//
// browser_eval and the back and forward paths of browser_history call
// ensureDebuggerChannel() or sendCdpCommand() at the start of their command;
// that first call attaches lazily. The channel detaches after thirty seconds
// without a CDP command, when its tab closes, and on every websocket reconnect.

function debuggerUnavailable(tabId: any, message: any, cause: any, reason: any) {
  const details: any = { tab_id: tabIdString(tabId), cause: cause };
  if (reason) details.reason = reason;
  return new ToolError('CAPABILITY_UNAVAILABLE', message, details);
}

async function requireDebuggableTab(tabId: any) {
  let tab = null;
  try {
    tab = await chrome.tabs.get(tabId);
  } catch (error) {
    throw new ToolError('BROWSER_GONE', 'the tab is no longer available for the debugger channel', {
      tab_id: tabIdString(tabId),
    });
  }
  if (isRestrictedUrl(tab.url)) {
    throw new ToolError('CAPABILITY_UNAVAILABLE', 'this page cannot be debugged: ' + String(tab.url), {
      tab_id: tabIdString(tabId),
      url: tab.url,
      cause: 'restricted_page',
    });
  }
  return tab;
}

function scheduleDebuggerIdleDetach(tabId: any) {
  const session = debuggerSessions.get(tabId);
  if (!session) return;
  if (session.idleTimer !== null) {
    clearTimeout(session.idleTimer);
    session.idleTimer = null;
  }
  const idleFor = Date.now() - session.lastUsedAt;
  const delay = Math.max(0, CDP_IDLE_TIMEOUT_MS - idleFor) + 25;
  session.idleTimer = setTimeout(() => {
    const live = debuggerSessions.get(tabId);
    if (!live) return;
    if (Date.now() - live.lastUsedAt < CDP_IDLE_TIMEOUT_MS) {
      scheduleDebuggerIdleDetach(tabId);
      return;
    }
    void detachDebuggerSession(tabId, 'idle_timeout');
  }, delay);
}

async function ensureDebuggerChannel(tabId: any) {
  const numeric = typeof tabId === 'number' ? tabId : Number(tabId);
  if (!Number.isInteger(numeric) || numeric < 0) {
    throw new ToolError('BAD_ARGS', 'the debugger channel needs a valid tab id');
  }

  const existing = debuggerSessions.get(numeric);
  if (existing) {
    existing.lastUsedAt = Date.now();
    scheduleDebuggerIdleDetach(numeric);
    return existing;
  }

  const pending = debuggerAttachPromises.get(numeric);
  if (pending) return pending;

  const attempt = (async () => {
    await requireDebuggableTab(numeric);

    // Another debugger (normally DevTools) owns the tab when a target is
    // already marked attached. Attaching anyway would only reject, so answer
    // with the same typed failure before touching the tab.
    let attachedElsewhere = null;
    try {
      const targets = await chrome.debugger.getTargets();
      attachedElsewhere = (targets || []).find(
        (target) => target && target.tabId === numeric && target.attached === true,
      );
    } catch (error) {
      log('debugger.getTargets failed', describeError(error));
    }
    if (attachedElsewhere) {
      throw debuggerUnavailable(
        numeric,
        'the debugger channel is unavailable because another debugger is already attached to this tab',
        'debugger_attached_elsewhere',
        'already attached',
      );
    }

    try {
      await withTimeout(chrome.debugger.attach({ tabId: numeric }, CDP_PROTOCOL_VERSION), CDP_ATTACH_TIMEOUT_MS);
    } catch (error) {
      const reason = describeError(error);
      const devtools = /another debugger|already attached|devtools|cannot attach|not allowed/i.test(reason);
      throw debuggerUnavailable(
        numeric,
        devtools
          ? 'the debugger channel is unavailable because DevTools or another debugger is attached to this tab'
          : 'the debugger channel could not attach to this tab',
        devtools ? 'debugger_attached_elsewhere' : 'attach_refused',
        reason,
      );
    }

    const session = { attachedAt: Date.now(), lastUsedAt: Date.now(), idleTimer: null };
    debuggerSessions.set(numeric, session);
    scheduleDebuggerIdleDetach(numeric);
    log('debugger attached', numeric);
    return session;
  })();

  debuggerAttachPromises.set(numeric, attempt);
  try {
    return await attempt;
  } finally {
    debuggerAttachPromises.delete(numeric);
  }
}

function touchDebuggerChannel(tabId: any) {
  const numeric = typeof tabId === 'number' ? tabId : Number(tabId);
  const session = debuggerSessions.get(numeric);
  if (!session) return false;
  session.lastUsedAt = Date.now();
  scheduleDebuggerIdleDetach(numeric);
  return true;
}

async function detachDebuggerSession(tabId: any, reason: any) {
  const numeric = typeof tabId === 'number' ? tabId : Number(tabId);
  const session = debuggerSessions.get(numeric);
  if (!session) return false;
  debuggerSessions.delete(numeric);
  markNetworkCaptureDetached(numeric);
  if (session.idleTimer !== null) {
    clearTimeout(session.idleTimer);
    session.idleTimer = null;
  }
  try {
    await withTimeout(chrome.debugger.detach({ tabId: numeric }), 2000);
  } catch (error) {
    log('debugger.detach failed', describeError(error));
  }
  log('debugger detached', numeric, reason || 'explicit');
  return true;
}

function clearDebuggerSession(tabId: any) {
  const numeric = typeof tabId === 'number' ? tabId : Number(tabId);
  const session = debuggerSessions.get(numeric);
  if (session && session.idleTimer !== null) clearTimeout(session.idleTimer);
  debuggerSessions.delete(numeric);
  debuggerAttachPromises.delete(numeric);
}

async function detachAllDebuggerSessions(reason: any) {
  const ids = Array.from(debuggerSessions.keys());
  for (let i = 0; i < ids.length; i += 1) {
    await detachDebuggerSession(ids[i], reason || 'explicit');
  }
}

async function sweepDebuggerSessions() {
  const now = Date.now();
  const stale: any[] = [];
  debuggerSessions.forEach((session, tabId) => {
    if (now - session.lastUsedAt >= CDP_IDLE_TIMEOUT_MS) stale.push(tabId);
  });
  for (let i = 0; i < stale.length; i += 1) {
    await detachDebuggerSession(stale[i], 'idle_timeout');
  }
}

async function sendCdpCommand(tabId: any, method: any, params: any) {
  if (typeof method !== 'string' || !method.trim()) {
    throw new ToolError('BAD_ARGS', 'a CDP method name is required');
  }
  if (params !== undefined && params !== null && (typeof params !== 'object' || Array.isArray(params))) {
    throw new ToolError('BAD_ARGS', 'CDP parameters must be an object');
  }
  const numeric = typeof tabId === 'number' ? tabId : Number(tabId);
  await ensureDebuggerChannel(numeric);
  touchDebuggerChannel(numeric);
  try {
    const value = await chrome.debugger.sendCommand({ tabId: numeric }, method.trim(), params || {});
    touchDebuggerChannel(numeric);
    return value === undefined ? null : value;
  } catch (error) {
    const reason = describeError(error);
    if (/not attached|detached|no tab/i.test(reason)) {
      clearDebuggerSession(numeric);
      throw debuggerUnavailable(
        numeric,
        'the debugger channel detached before the CDP command completed',
        'debugger_detached',
        reason,
      );
    }
    throw new ToolError('INTERNAL', 'the CDP command failed: ' + reason, {
      tab_id: tabIdString(numeric),
      method: method.trim(),
    });
  }
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
    void sweepDebuggerSessions();
    const previousCdp = cdpFingerprint(state.cdp);
    void refreshCdpDetection('alarm', true)
      .then((result) => {
        if (cdpFingerprint(result) !== previousCdp) {
          void sendCapabilitiesEvent('cdp_alarm');
        }
      })
      .catch((error) => {
        log('cdp alarm detection failed', describeError(error));
      });
    const exists = await ensureOffscreen();
    if (!exists) {
      updateStatus('disconnected', 'offscreen document unavailable');
      return;
    }
    const status = await sendToOffscreen({ type: 'corvus.status_request' });
    if (status && typeof status.url === 'string') {
      const reportedUrl = DAEMON_URL_HELPER.normalize(status.url);
      if (reportedUrl) state.wsUrl = reportedUrl;
    }
    const reported = status && typeof status.status === 'string' ? normalizeStatus(status.status) : 'disconnected';
    updateStatus(reported, status && status.lastError ? status.lastError : null);
    if (reported !== 'connected') {
      await sendToOffscreen({ type: 'corvus.connect', url: state.wsUrl, reason: 'watchdog' });
    }
  } catch (error) {
    const message = describeError(error);
    log('watchdog tick failed', message);
    updateStatus('disconnected', message);
    void ensureOffscreen();
  }
}

async function doBoot(reason: any) {
  log('boot', reason || '');
  await stateReady;
  await ensureCdpDetection(true);
  await ensureWatchdog();

  const exists = await ensureOffscreen();
  if (!exists) {
    updateStatus('disconnected', 'offscreen document unavailable');
    return;
  }

  try {
    await sendToOffscreen({ type: 'corvus.connect', url: state.wsUrl, reason: reason || 'boot' });
    const status = await sendToOffscreen({ type: 'corvus.status_request' });
    if (status && typeof status.url === 'string') {
      const reportedUrl = DAEMON_URL_HELPER.normalize(status.url);
      if (reportedUrl) state.wsUrl = reportedUrl;
    }
    if (status && typeof status.status === 'string') {
      updateStatus(normalizeStatus(status.status), status.lastError ? status.lastError : null);
    }
  } catch (error) {
    updateStatus('disconnected', describeError(error));
  }
}

function boot(reason: any) {
  if (bootPromise) return bootPromise;
  bootPromise = doBoot(reason)
    .catch((error) => {
      log('boot failed', describeError(error));
    })
    .finally(() => {
      bootPromise = null;
    });
  return bootPromise;
}

// ---------------------------------------------------------------------------
// Message plumbing
// ---------------------------------------------------------------------------

function handleOffscreenMessage(message: any) {
  if (!message || typeof message !== 'object') return;
  switch (message.type) {
    case 'status':
      if (typeof message.url === 'string') {
        const reportedUrl = DAEMON_URL_HELPER.normalize(message.url);
        if (reportedUrl) state.wsUrl = reportedUrl;
      }
      updateStatus(message.status, message.lastError ? message.lastError : null);
      return;
    case 'socket_open':
      // A fresh websocket, including every reconnect and watchdog restart, is
      // a new transport generation; no debugger session survives it.
      void detachAllDebuggerSessions('reconnect');
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

async function handleUiMessage(message: any) {
  switch (message.type) {
    case 'corvus.get_state':
      await stateReady;
      state.wsUrl = await DAEMON_URL_HELPER.readDaemonUrl();
      state.cdpEndpoint = await CDP_ENDPOINT_HELPER.readCdpEndpoint();
      await ensureCdpDetection(false);
      return { ok: true, state: buildUiState() };
    case 'corvus.cdp_refresh':
      await stateReady;
      state.cdpEndpoint = await CDP_ENDPOINT_HELPER.readCdpEndpoint();
      await ensureCdpDetection(true);
      await sendCapabilitiesEvent('cdp_refresh');
      return { ok: true, state: buildUiState() };
    case 'corvus.cdp_save':
      await stateReady;
      try {
        state.cdpEndpoint = await CDP_ENDPOINT_HELPER.writeCdpEndpoint(message.endpoint);
      } catch (error) {
        return { ok: false, error: describeError(error) };
      }
      await ensureCdpDetection(true);
      await sendCapabilitiesEvent('cdp_save');
      return { ok: true, state: buildUiState() };
    case 'corvus.cdp_reset':
      await stateReady;
      try {
        state.cdpEndpoint = await CDP_ENDPOINT_HELPER.resetCdpEndpoint();
      } catch (error) {
        return { ok: false, error: describeError(error) };
      }
      await ensureCdpDetection(true);
      await sendCapabilitiesEvent('cdp_reset');
      return { ok: true, state: buildUiState() };
    case 'corvus.daemon_save':
      await stateReady;
      try {
        state.wsUrl = await DAEMON_URL_HELPER.writeDaemonUrl(message.url);
      } catch (error) {
        return { ok: false, error: describeError(error) };
      }
      await sendToOffscreen({ type: 'corvus.connect', url: state.wsUrl, reason: 'ui' });
      return { ok: true, state: buildUiState() };
    case 'corvus.daemon_reset':
      await stateReady;
      try {
        state.wsUrl = await DAEMON_URL_HELPER.resetDaemonUrl();
      } catch (error) {
        return { ok: false, error: describeError(error) };
      }
      await sendToOffscreen({ type: 'corvus.connect', url: state.wsUrl, reason: 'ui' });
      return { ok: true, state: buildUiState() };
    case 'corvus.reconnect':
      await stateReady;
      await sendToOffscreen({ type: 'corvus.connect', url: state.wsUrl, reason: 'ui' });
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

chrome.storage.onChanged.addListener((changes, areaName) => {
  if (areaName !== 'local') return;
  const change = changes[STORAGE.daemonUrl];
  if (!change) return;

  const next =
    change.newValue === undefined ? DAEMON_URL_HELPER.DEFAULT_DAEMON_URL : DAEMON_URL_HELPER.normalize(change.newValue);
  if (!next) {
    log('ignoring invalid daemon URL storage change');
    return;
  }

  state.wsUrl = next;
  void sendToOffscreen({ type: 'corvus.connect', url: next, reason: 'storage' }).catch((error) => {
    log('daemon URL reconnect failed', describeError(error));
  });
});

chrome.alarms.onAlarm.addListener((alarm) => {
  if (!alarm || alarm.name !== ALARM_WATCHDOG) return;
  void watchdogTick();
});

chrome.debugger.onDetach.addListener((source, reason) => {
  const tabId = source && typeof source.tabId === 'number' ? source.tabId : null;
  if (tabId === null) return;
  handleDebuggerDetach(tabId, reason);
});

chrome.debugger.onEvent.addListener((source, method, params) => {
  try {
    handleDebuggerEvent(source, method, params);
  } catch (error) {
    log('debugger event failed', method, describeError(error));
  }
});

chrome.tabs.onRemoved.addListener((tabId) => {
  // The browser drops the debugger with the tab; ask explicitly and clear the
  // local session so no timer or pending attach survives the tab.
  void detachDebuggerSession(tabId, 'tab_closed');
  clearDebuggerSession(tabId);
  cleanupNetworkForTab(tabId);
});

chrome.runtime.onInstalled.addListener(() => {
  void boot('installed');
});

chrome.runtime.onStartup.addListener(() => {
  void boot('startup');
});

self.addEventListener('online', () => {
  void sendToOffscreen({ type: 'corvus.connect', url: state.wsUrl, reason: 'network_online' }).catch(() => {});
});

export { handleUiMessage };

void boot('service_worker_start');
