/**
 * P2 provider-only service-worker runtime.
 *
 * The service worker owns provider state and executes capability actions. The
 * offscreen document owns the encrypted socket. No action here uses a debugger,
 * legacy plaintext bridge or the removed 1.x tool catalogue.
 */
import {
  ProviderActionError,
  dispatchProviderAction,
  parseProviderAction,
  providerCapabilities,
  providerSystemInfo,
  type ProviderCaptured,
  type ProviderNavigated,
  type ProviderTabInfo,
} from './provider-actions';
import {
  DAEMON_URL_STORAGE_KEY,
  PAIRING_TOKEN_STORAGE_KEY,
  providerBaseFromStorageValue,
  providerEndpointFromBase,
} from '../shared/provider/endpoint';
import { normalizePairingToken } from '../shared/provider/token';
import { readStorageValue, removeStorageValue, writeStorageValue } from '../shared/storage/adapter';

export type ProviderUiStatus = 'pairing_required' | 'connecting' | 'connected' | 'disconnected' | 'error';

export interface ProviderUiBrowser {
  family: string;
  version: string;
  profile: string;
}

export interface ProviderUiState {
  status: ProviderUiStatus;
  connected: boolean;
  base_url: string;
  token_saved: boolean;
  provider_id: string | null;
  server_id: string | null;
  session_id: string | null;
  browser: ProviderUiBrowser | null;
  last_error: { code: string; message: string } | null;
  version: string;
  capabilities: Array<{ id: string; version: number; actions: string[] }>;
}

export type ProviderUiReply = { ok: true; state: ProviderUiState } | { ok: false; error: string };

type ProviderUiRequest =
  | { kind: 'get_state' }
  | { kind: 'save_and_connect'; base_url: string; token?: string }
  | { kind: 'connect' }
  | { kind: 'disconnect' };

const STATE_STORAGE_KEY = 'provider.uiState';
const OFFSCREEN_URL = 'offscreen.html';
const DEFAULT_UI_STATE: ProviderUiState = {
  status: 'disconnected',
  connected: false,
  base_url: 'ws://127.0.0.1:7330',
  token_saved: false,
  provider_id: null,
  server_id: null,
  session_id: null,
  browser: null,
  last_error: null,
  version: '1.7.0',
  capabilities: providerCapabilities(),
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

async function readSessionState(): Promise<Partial<ProviderUiState> | null> {
  try {
    if (typeof chrome !== 'undefined' && chrome.storage && chrome.storage.session) {
      const stored = await chrome.storage.session.get(STATE_STORAGE_KEY);
      const value = stored ? stored[STATE_STORAGE_KEY] : undefined;
      return isRecord(value) ? (value as Partial<ProviderUiState>) : null;
    }
  } catch {
    // Storage may be unavailable in tests or during shutdown.
  }
  return null;
}

async function writeSessionState(state: ProviderUiState): Promise<void> {
  try {
    if (typeof chrome !== 'undefined' && chrome.storage && chrome.storage.session) {
      await chrome.storage.session.set({ [STATE_STORAGE_KEY]: state });
    }
  } catch {
    // Runtime state can be recreated by a reconnect.
  }
}

async function readToken(): Promise<string | null> {
  return normalizePairingToken(await readStorageValue(PAIRING_TOKEN_STORAGE_KEY));
}

async function readBase(): Promise<string> {
  return providerBaseFromStorageValue(await readStorageValue(DAEMON_URL_STORAGE_KEY));
}

export async function getProviderUiState(): Promise<ProviderUiState> {
  const [stored, token, base] = await Promise.all([readSessionState(), readToken(), readBase()]);
  const version =
    typeof chrome !== 'undefined' && chrome.runtime?.getManifest ? chrome.runtime.getManifest().version : '1.7.0';
  return {
    ...DEFAULT_UI_STATE,
    ...(stored ?? {}),
    base_url: base,
    token_saved: token !== null,
    version,
    capabilities: providerCapabilities(),
  };
}

async function updateState(patch: Partial<ProviderUiState>): Promise<ProviderUiState> {
  const current = await getProviderUiState();
  const next: ProviderUiState = { ...current, ...patch };
  next.connected = next.status === 'connected';
  await writeSessionState(next);
  return next;
}

export async function ensureOffscreenDocument(): Promise<void> {
  if (typeof chrome === 'undefined' || !chrome.offscreen || !chrome.runtime?.getContexts) return;
  const existing = await chrome.runtime.getContexts({
    contextTypes: ['OFFSCREEN_DOCUMENT' as chrome.runtime.ContextType],
    documentUrls: [chrome.runtime.getURL(OFFSCREEN_URL)],
  });
  if (existing.length > 0) return;
  try {
    await chrome.offscreen.createDocument({
      url: OFFSCREEN_URL,
      reasons: ['WORKERS'],
      justification: 'Maintain the encrypted provider connection.',
    });
  } catch {
    // A concurrent request may have created the document first.
  }
}

async function sendToOffscreen(message: Record<string, unknown>): Promise<unknown> {
  await ensureOffscreenDocument();
  try {
    return await chrome.runtime.sendMessage({ target: 'offscreen', ...message });
  } catch {
    return undefined;
  }
}

async function connectOffscreen(): Promise<void> {
  const [base, token] = await Promise.all([readBase(), readToken()]);
  await sendToOffscreen({ type: 'provider.connect', base_url: base, token });
}

async function disconnectOffscreen(): Promise<void> {
  await sendToOffscreen({ type: 'provider.disconnect' });
}

function tabNumber(value: unknown): number | null {
  if (typeof value === 'number' && Number.isInteger(value) && value > 0) return value;
  if (typeof value === 'string' && /^[1-9][0-9]*$/.test(value)) {
    const parsed = Number(value);
    return Number.isSafeInteger(parsed) ? parsed : null;
  }
  return null;
}

function toTabInfo(tab: chrome.tabs.Tab): ProviderTabInfo | null {
  if (typeof tab.id !== 'number') return null;
  return {
    tab_id: String(tab.id),
    window_id: typeof tab.windowId === 'number' ? tab.windowId : 0,
    title: typeof tab.title === 'string' ? tab.title : '',
    url: typeof tab.url === 'string' ? tab.url : '',
    active: tab.active === true,
  };
}

async function listTabs(): Promise<ProviderTabInfo[]> {
  const tabs = await chrome.tabs.query({});
  return tabs
    .map(toTabInfo)
    .filter((tab): tab is ProviderTabInfo => tab !== null)
    .sort((a, b) => (a.window_id === b.window_id ? Number(a.tab_id) - Number(b.tab_id) : a.window_id - b.window_id))
    .slice(0, 200);
}

function allowedNavigationUrl(raw: string): URL {
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    throw new ProviderActionError('BAD_ARGS', 'invalid navigation URL');
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:' && raw !== 'about:blank') {
    throw new ProviderActionError('BAD_ARGS', 'navigation URL scheme is not supported');
  }
  return url;
}

async function openTab(url: string, active: boolean): Promise<ProviderTabInfo> {
  const target = allowedNavigationUrl(url);
  try {
    const tab = await chrome.tabs.create({
      url: target.protocol === 'about:' ? 'about:blank' : target.toString(),
      active,
    });
    const info = toTabInfo(tab);
    if (!info) throw new ProviderActionError('INTERNAL', 'the browser did not create a tab');
    if (!info.url) info.url = target.protocol === 'about:' ? 'about:blank' : target.toString();
    return info;
  } catch (error) {
    if (error instanceof ProviderActionError) throw error;
    throw new ProviderActionError('INTERNAL', 'the browser could not open a tab');
  }
}

async function closeTab(tabId: string): Promise<void> {
  const numeric = tabNumber(tabId);
  if (numeric === null) throw new ProviderActionError('BAD_ARGS', 'invalid tab id');
  try {
    await chrome.tabs.remove(numeric);
  } catch {
    throw new ProviderActionError('NOT_FOUND', 'the tab does not exist');
  }
}

async function activeTab(): Promise<chrome.tabs.Tab | null> {
  const focused = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  if (focused.length > 0 && focused[0]) return focused[0];
  const all = await chrome.tabs.query({});
  const sorted = all
    .filter((tab) => typeof tab.id === 'number')
    .sort((a, b) =>
      (a.windowId ?? 0) === (b.windowId ?? 0) ? (a.id ?? 0) - (b.id ?? 0) : (a.windowId ?? 0) - (b.windowId ?? 0),
    );
  return sorted[0] ?? null;
}

async function resolveTab(tabId: string | undefined): Promise<chrome.tabs.Tab> {
  if (tabId !== undefined) {
    const numeric = tabNumber(tabId);
    if (numeric === null) throw new ProviderActionError('BAD_ARGS', 'invalid tab id');
    try {
      return await chrome.tabs.get(numeric);
    } catch {
      throw new ProviderActionError('NOT_FOUND', 'the tab does not exist');
    }
  }
  const tab = await activeTab();
  if (!tab || typeof tab.id !== 'number') throw new ProviderActionError('NOT_FOUND', 'no tab is available');
  return tab;
}

async function waitForDocument(
  tabId: number,
  waitUntil: 'load' | 'domcontentloaded',
  timeoutMs: number,
  previousUrl = '',
  expectedUrl = '',
): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const current = await chrome.tabs.get(tabId);
      if (previousUrl && current.url === previousUrl && expectedUrl !== previousUrl) {
        await new Promise((resolve) => setTimeout(resolve, 100));
        continue;
      }
      const results = await chrome.scripting.executeScript({
        target: { tabId },
        func: () => document.readyState,
      });
      const ready = results[0]?.result;
      if (waitUntil === 'load' ? ready === 'complete' : ready === 'interactive' || ready === 'complete') return true;
    } catch {
      // The target may still be navigating or may not accept scripts yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  return false;
}

async function navigate(
  tabId: string | undefined,
  url: string,
  waitUntil: 'load' | 'domcontentloaded',
  timeoutMs: number,
): Promise<ProviderNavigated> {
  const target = allowedNavigationUrl(url);
  const tab = await resolveTab(tabId);
  const numeric = tab.id;
  if (typeof numeric !== 'number') throw new ProviderActionError('NOT_FOUND', 'the tab does not exist');
  const finalUrl = target.protocol === 'about:' ? 'about:blank' : target.toString();
  const previousUrl = typeof tab.url === 'string' ? tab.url : '';
  try {
    await chrome.tabs.update(numeric, { url: finalUrl });
  } catch {
    throw new ProviderActionError('NAVIGATION_FAILED', 'the browser rejected the navigation');
  }
  const loaded = await waitForDocument(numeric, waitUntil, timeoutMs, previousUrl, finalUrl);
  if (!loaded) throw new ProviderActionError('TIMEOUT', 'the page did not finish loading in time');
  let updated: chrome.tabs.Tab;
  try {
    updated = await chrome.tabs.get(numeric);
  } catch {
    throw new ProviderActionError('NAVIGATION_FAILED', 'the tab closed during navigation');
  }
  return {
    tab_id: String(numeric),
    url: typeof updated.url === 'string' ? updated.url : finalUrl,
    title: typeof updated.title === 'string' ? updated.title : undefined,
    status: 'loaded',
  };
}

async function contentAction(
  tabId: string | undefined,
  action: string,
  args: Record<string, unknown>,
): Promise<unknown> {
  const tab = await resolveTab(tabId);
  const numeric = tab.id;
  if (typeof numeric !== 'number') throw new ProviderActionError('NOT_FOUND', 'the tab does not exist');
  const send = async (): Promise<unknown> => {
    const reply = await chrome.tabs.sendMessage(numeric, { type: 'corvus.provider_action', action, args });
    if (isRecord(reply) && reply.ok === true) return reply.value;
    if (isRecord(reply) && reply.ok === false && isRecord(reply.error)) {
      const code = String(reply.error.code ?? 'INTERNAL');
      const message = String(reply.error.message ?? 'page action failed');
      throw new ProviderActionError(code as never, message);
    }
    throw new ProviderActionError('INTERNAL', 'the page did not answer the action');
  };
  const withTabId = (value: unknown): unknown => {
    if (action === 'page.read' || action === 'page.snapshot' || action === 'input.click' || action === 'input.type') {
      if (isRecord(value)) return { ...value, tab_id: String(numeric) };
    }
    return value;
  };
  try {
    return withTabId(await send());
  } catch (error) {
    if (error instanceof ProviderActionError) throw error;
    try {
      await chrome.scripting.executeScript({ target: { tabId: numeric }, files: ['content.js'] });
      return withTabId(await send());
    } catch (retryError) {
      if (retryError instanceof ProviderActionError) throw retryError;
      throw new ProviderActionError('INTERNAL', 'the page is not scriptable');
    }
  }
}

function decodeBase64(value: string): Uint8Array {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return bytes;
}

function imageSize(bytes: Uint8Array): { width: number; height: number } {
  if (bytes.length >= 24 && bytes[0] === 0x89 && bytes[1] === 0x50 && bytes[2] === 0x4e && bytes[3] === 0x47) {
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    return { width: view.getUint32(16, false), height: view.getUint32(20, false) };
  }
  if (bytes.length >= 4 && bytes[0] === 0xff && bytes[1] === 0xd8) {
    let offset = 2;
    while (offset + 9 < bytes.length) {
      if (bytes[offset] !== 0xff) {
        offset += 1;
        continue;
      }
      const marker = bytes[offset + 1];
      offset += 2;
      if (marker === 0xd8 || marker === 0xd9) continue;
      if (offset + 1 >= bytes.length) break;
      const length = (bytes[offset] << 8) | bytes[offset + 1];
      if (length < 2) break;
      if (
        (marker >= 0xc0 && marker <= 0xc3) ||
        (marker >= 0xc5 && marker <= 0xc7) ||
        (marker >= 0xc9 && marker <= 0xcb) ||
        (marker >= 0xcd && marker <= 0xcf)
      ) {
        if (offset + 7 < bytes.length) {
          return {
            width: (bytes[offset + 5] << 8) | bytes[offset + 6],
            height: (bytes[offset + 3] << 8) | bytes[offset + 4],
          };
        }
      }
      offset += length;
    }
  }
  return { width: 0, height: 0 };
}

async function captureViewport(
  tabId: string | undefined,
  format: 'png' | 'jpeg',
  quality?: number,
): Promise<ProviderCaptured> {
  const tab = await resolveTab(tabId);
  const numeric = tab.id;
  const windowId = tab.windowId;
  if (typeof numeric !== 'number' || typeof windowId !== 'number') {
    throw new ProviderActionError('NOT_FOUND', 'the tab does not exist');
  }
  if (tab.active !== true) {
    try {
      await chrome.tabs.update(numeric, { active: true });
    } catch {
      throw new ProviderActionError('INTERNAL', 'the browser could not activate the tab');
    }
  }
  let dataUrl: string;
  try {
    const captureOptions = format === 'jpeg' && quality !== undefined ? { format, quality } : { format };
    dataUrl = await chrome.tabs.captureVisibleTab(windowId, captureOptions);
  } catch {
    throw new ProviderActionError('CAPABILITY_UNAVAILABLE', 'the viewport cannot be captured');
  }
  const comma = dataUrl.indexOf(',');
  const data = comma >= 0 ? dataUrl.slice(comma + 1) : '';
  if (!data) throw new ProviderActionError('INTERNAL', 'the browser returned an empty capture');
  const size = imageSize(decodeBase64(data));
  return {
    tab_id: String(numeric),
    url: typeof tab.url === 'string' ? tab.url : '',
    format,
    mime_type: format === 'jpeg' ? 'image/jpeg' : 'image/png',
    width: size.width,
    height: size.height,
    data,
  };
}

function actionDeps() {
  return {
    listTabs,
    openTab,
    closeTab,
    navigate,
    contentAction,
    captureViewport,
    systemInfo: providerSystemInfo,
  };
}

export async function handleProviderAction(action: unknown, args: unknown): Promise<unknown> {
  try {
    const parsed = parseProviderAction(action, args);
    return await dispatchProviderAction(parsed, actionDeps());
  } catch (error) {
    if (error instanceof ProviderActionError) throw error;
    throw new ProviderActionError('INTERNAL', 'provider action failed');
  }
}

interface RuntimeStateMessage {
  status?: string;
  error?: { code?: string; message?: string } | null;
  base_url?: string;
  provider_id?: string | null;
  server_id?: string | null;
  session_id?: string | null;
  browser?: ProviderUiBrowser | null;
}

function statusFromRuntime(value: unknown): ProviderUiStatus {
  return value === 'connecting' || value === 'connected' || value === 'disconnected' || value === 'pairing_required'
    ? value
    : 'error';
}

async function acceptRuntimeState(message: RuntimeStateMessage): Promise<void> {
  const status = statusFromRuntime(message.status);
  const error =
    message.error && typeof message.error.code === 'string'
      ? {
          code: message.error.code,
          message: typeof message.error.message === 'string' ? message.error.message : 'provider error',
        }
      : null;
  await updateState({
    status,
    base_url: typeof message.base_url === 'string' ? message.base_url : await readBase(),
    provider_id: typeof message.provider_id === 'string' ? message.provider_id : null,
    server_id: typeof message.server_id === 'string' ? message.server_id : null,
    session_id: typeof message.session_id === 'string' ? message.session_id : null,
    browser: message.browser && typeof message.browser === 'object' ? message.browser : null,
    last_error: error,
  });
}

async function saveAndConnect(base: unknown, token: unknown): Promise<ProviderUiReply> {
  const endpoint = providerEndpointFromBase(base);
  if (!endpoint) return { ok: false, error: 'Enter a loopback ws:// URL, for example ws://127.0.0.1:7330.' };
  const baseUrl = String(base).trim().replace(/\/$/, '');
  if (token !== undefined && token !== '') {
    const normalized = normalizePairingToken(token);
    if (!normalized) return { ok: false, error: 'Enter the 43-character pairing token.' };
    try {
      await writeStorageValue(PAIRING_TOKEN_STORAGE_KEY, normalized);
    } catch {
      return { ok: false, error: 'The pairing token could not be saved.' };
    }
  } else if (!(await readToken())) {
    return { ok: false, error: 'Enter the pairing token before connecting.' };
  }
  try {
    await writeStorageValue(DAEMON_URL_STORAGE_KEY, baseUrl);
  } catch {
    return { ok: false, error: 'The local URL could not be saved.' };
  }
  await connectOffscreen();
  const state = await getProviderUiState();
  return { ok: true, state };
}

export async function handleProviderUiRequest(request: unknown): Promise<ProviderUiReply> {
  if (!isRecord(request) || typeof request.kind !== 'string') return { ok: false, error: 'invalid request' };
  switch (request.kind as ProviderUiRequest['kind']) {
    case 'get_state':
      return { ok: true, state: await getProviderUiState() };
    case 'save_and_connect':
      return saveAndConnect(request.base_url, request.token);
    case 'connect': {
      if (!(await readToken())) return { ok: false, error: 'Enter the pairing token before connecting.' };
      await connectOffscreen();
      return { ok: true, state: await getProviderUiState() };
    }
    case 'disconnect':
      await disconnectOffscreen();
      return { ok: true, state: await updateState({ status: 'disconnected', last_error: null }) };
    default:
      return { ok: false, error: 'invalid request' };
  }
}

export async function clearPairingToken(): Promise<void> {
  await removeStorageValue(PAIRING_TOKEN_STORAGE_KEY);
}

export function installProviderRuntime(): void {
  chrome.runtime.onMessage.addListener((message: unknown, _sender, sendResponse) => {
    if (!isRecord(message)) return false;
    if (message.source === 'corvus.offscreen') {
      if (message.type === 'provider.state') {
        void acceptRuntimeState(message as RuntimeStateMessage).then(() => sendResponse({ ok: true }));
        return true;
      }
      if (message.type === 'provider.action') {
        void handleProviderAction(message.action, message.args).then(
          (value) => sendResponse({ ok: true, value }),
          (error: unknown) => {
            const code = error instanceof ProviderActionError ? error.code : 'INTERNAL';
            const detail = error instanceof ProviderActionError ? error.message : 'provider action failed';
            sendResponse({ ok: false, error: { code, message: detail } });
          },
        );
        return true;
      }
    }
    if (message.type === 'corvus.provider.ui') {
      void handleProviderUiRequest(message.request).then(sendResponse);
      return true;
    }
    return false;
  });

  chrome.runtime.onStartup?.addListener(() => {
    void (async () => {
      if (await readToken()) await connectOffscreen();
    })();
  });
  chrome.runtime.onInstalled?.addListener(() => {
    void ensureOffscreenDocument();
  });
  void ensureOffscreenDocument();
}
