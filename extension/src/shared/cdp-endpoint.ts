/**
 * CDP endpoint helper.
 *
 * Detection is a read-only HTTP probe of /json/version. The extension never
 * attaches chrome.debugger to discover an endpoint, and nothing here throws for
 * an unavailable browser: callers receive available:false plus a short reason.
 */

import { hasStorage, readStorageValue, removeStorageValue, writeStorageValue } from './storage/adapter';

export const DEFAULT_CDP_ENDPOINT = 'auto';
export const CDP_ENDPOINT_STORAGE_KEY = 'corvus.cdpEndpoint';
export const CANDIDATE_PORTS: readonly number[] = Object.freeze([9222, 9223, 9333]);
export const PROBE_TIMEOUT_MS = 800;
const PROBE_PATH = '/json/version';
const MAX_REASON_LENGTH = 160;

export type BrowserFamily = 'chrome' | 'chromium' | 'edge' | 'brave' | 'opera' | 'vivaldi' | 'unknown';
export type CdpSource = 'auto' | 'configured';

export interface CdpValidation {
  ok: boolean;
  value?: string;
  error?: string;
}

export interface CdpProbeUnavailable {
  available: false;
  endpoint: string | null;
  webSocketDebuggerUrl: null;
  browser: null;
  browserFamily: null;
  expectedBrowserFamily: BrowserFamily | null;
  familyMatch: boolean | null;
  matchKind?: null;
  protocolVersion: null;
  source: CdpSource;
  reason: string;
}

export interface CdpProbeAvailable {
  available: true;
  endpoint: string;
  webSocketDebuggerUrl: string;
  browser: string;
  browserFamily: BrowserFamily;
  expectedBrowserFamily: BrowserFamily;
  familyMatch: boolean | null;
  matchKind: 'exact' | 'chromium' | null;
  protocolVersion: string | null;
  source: CdpSource;
  reason: null;
}

export type CdpProbeResult = CdpProbeUnavailable | CdpProbeAvailable;

const CHROMIUM_BRANDS: readonly BrowserFamily[] = Object.freeze(['chrome', 'chromium']);

function currentNavigator(): Navigator | null {
  if (typeof navigator !== 'undefined' && navigator) return navigator;
  if (typeof self !== 'undefined' && self && 'navigator' in self) return self.navigator;
  return null;
}

interface BrandList {
  brands?: Array<{ brand?: string }>;
}

function brandNames(nav: Navigator | null): string[] {
  const data = (nav as (Navigator & { userAgentData?: BrandList }) | null)?.userAgentData;
  const brands = data && Array.isArray(data.brands) ? data.brands : [];
  return brands.map((item) => String(item && item.brand ? item.brand : '').toLowerCase());
}

export function detectBrowserFamily(nav?: Navigator | null): BrowserFamily {
  const target = nav || currentNavigator();
  const names = brandNames(target);
  const hasBrand = (needle: string) => names.some((name) => name.indexOf(needle) !== -1);

  if (hasBrand('microsoft edge') || hasBrand('edge')) return 'edge';
  if (hasBrand('brave')) return 'brave';
  if (hasBrand('opera') || hasBrand('opr')) return 'opera';
  if (hasBrand('vivaldi')) return 'vivaldi';
  if (hasBrand('headlesschrome')) return 'chrome';
  if (hasBrand('google chrome')) return 'chrome';
  if (hasBrand('chromium')) return 'chromium';
  if (hasBrand('chrome')) return 'chrome';

  const ua = String((target && target.userAgent) || '');
  if (/Edg\//.test(ua) || /EdgA\//.test(ua) || /EdgiOS\//.test(ua)) return 'edge';
  if (/OPR\//.test(ua)) return 'opera';
  if (/Vivaldi\//.test(ua)) return 'vivaldi';
  if (/Brave\//.test(ua)) return 'brave';
  if (/HeadlessChrome\//.test(ua)) return 'chrome';
  if (/Chromium\//.test(ua)) return 'chromium';
  if (/Chrome\//.test(ua)) return 'chrome';
  return 'unknown';
}

export function browserFamilyFromProduct(product: string | null | undefined): BrowserFamily {
  const value = String(product || '')
    .trim()
    .toLowerCase();
  if (!value) return 'unknown';
  if (value.indexOf('edg/') === 0 || value.indexOf('edge/') === 0 || value.indexOf('microsoft edge') === 0)
    return 'edge';
  if (value.indexOf('brave/') === 0) return 'brave';
  if (value.indexOf('opr/') === 0 || value.indexOf('opera/') === 0) return 'opera';
  if (value.indexOf('vivaldi/') === 0) return 'vivaldi';
  if (value.indexOf('headlesschrome/') === 0) return 'chrome';
  if (value.indexOf('chrome/') === 0 || value.indexOf('google chrome/') === 0) return 'chrome';
  if (value.indexOf('chromium/') === 0) return 'chromium';
  return 'unknown';
}

function isChromiumBrand(family: BrowserFamily): boolean {
  return CHROMIUM_BRANDS.indexOf(family) !== -1;
}

export function compareBrowserFamily(
  expected: BrowserFamily | null,
  actual: BrowserFamily | null,
): { match: boolean | null; kind: 'exact' | 'chromium' | null } {
  if (!expected || expected === 'unknown' || !actual || actual === 'unknown') {
    return { match: null, kind: null };
  }
  if (expected === actual) return { match: true, kind: 'exact' };
  if (isChromiumBrand(expected) && isChromiumBrand(actual)) return { match: true, kind: 'chromium' };
  return { match: false, kind: null };
}

function shorten(value: string | null | undefined): string | null {
  if (typeof value !== 'string') return null;
  const text = value.replace(/\s+/g, ' ').trim();
  return text ? text.slice(0, MAX_REASON_LENGTH) : null;
}

function isLoopbackHostname(hostname: string): boolean {
  const host = hostname.toLowerCase();
  return host === '127.0.0.1' || host === 'localhost' || host === '[::1]' || host === '::1';
}

export function validateCdpEndpoint(value: unknown): CdpValidation {
  if (value === undefined || value === null) {
    return { ok: true, value: DEFAULT_CDP_ENDPOINT };
  }
  if (typeof value !== 'string') {
    return { ok: false, error: 'Enter auto or an http:// loopback CDP endpoint.' };
  }

  const text = value.trim();
  if (!text || text.toLowerCase() === 'auto') {
    return { ok: true, value: DEFAULT_CDP_ENDPOINT };
  }

  let url: URL;
  try {
    url = new URL(text);
  } catch {
    return { ok: false, error: 'Enter auto or a valid http:// loopback endpoint.' };
  }

  if (url.protocol !== 'http:') {
    return { ok: false, error: 'Use http://; CDP detection does not use HTTPS or WebSocket URLs.' };
  }
  if (url.username || url.password) {
    return { ok: false, error: 'Remove the username and password from the CDP endpoint.' };
  }
  if (!isLoopbackHostname(url.hostname)) {
    return { ok: false, error: 'Use a loopback host: 127.0.0.1, localhost or [::1].' };
  }
  if (!url.port) {
    return { ok: false, error: 'Include the CDP port, for example http://127.0.0.1:9222.' };
  }

  const port = Number(url.port);
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    return { ok: false, error: 'Use a port between 1 and 65535.' };
  }
  if (url.pathname !== '/' && url.pathname !== '') {
    return { ok: false, error: 'Use http://host:port; paths are not supported.' };
  }
  if (url.search || url.hash) {
    return { ok: false, error: 'Remove the query string and fragment; only http://host:port is supported.' };
  }

  return { ok: true, value: 'http://' + url.hostname.toLowerCase() + ':' + String(port) };
}

export function normalizeCdpEndpoint(value: unknown): string | null {
  const result = validateCdpEndpoint(value);
  return result.ok && result.value ? result.value : null;
}

export class CdpConfigurationError extends Error {
  readonly code = 'INVALID_CDP_ENDPOINT';
  constructor(message: string) {
    super(message);
    this.name = 'CdpConfigurationError';
  }
}

function requireValidCdpEndpoint(value: unknown): string {
  const result = validateCdpEndpoint(value);
  if (!result.ok || !result.value) throw new CdpConfigurationError(result.error || 'Invalid CDP endpoint');
  return result.value;
}

export async function readCdpEndpoint(): Promise<string> {
  if (!hasStorage()) return DEFAULT_CDP_ENDPOINT;
  const stored = await readStorageValue(CDP_ENDPOINT_STORAGE_KEY);
  return normalizeCdpEndpoint(stored) || DEFAULT_CDP_ENDPOINT;
}

export async function writeCdpEndpoint(value: unknown): Promise<string> {
  const normalized = requireValidCdpEndpoint(value);
  await writeStorageValue(CDP_ENDPOINT_STORAGE_KEY, normalized);
  return normalized;
}

export async function resetCdpEndpoint(): Promise<string> {
  await removeStorageValue(CDP_ENDPOINT_STORAGE_KEY);
  return DEFAULT_CDP_ENDPOINT;
}

function unavailable(
  endpoint: string | null,
  source: CdpSource,
  reason: string,
  expectedBrowserFamily: BrowserFamily | null,
  familyMatch?: boolean | null,
): CdpProbeUnavailable {
  return {
    available: false,
    endpoint: endpoint || null,
    webSocketDebuggerUrl: null,
    browser: null,
    browserFamily: null,
    expectedBrowserFamily: expectedBrowserFamily || null,
    familyMatch: typeof familyMatch === 'boolean' ? familyMatch : null,
    protocolVersion: null,
    source: source || 'auto',
    reason: shorten(reason) || 'CDP endpoint unavailable',
  };
}

async function probeOne(
  endpoint: string,
  source: CdpSource,
  expectedBrowserFamily: BrowserFamily,
): Promise<CdpProbeResult> {
  const fail = (reason: string, familyMatch?: boolean | null) =>
    unavailable(endpoint, source, reason, expectedBrowserFamily, familyMatch);

  if (typeof fetch !== 'function') return fail('fetch is unavailable');

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), PROBE_TIMEOUT_MS);
  let response: Response;
  try {
    response = await fetch(endpoint + PROBE_PATH, {
      method: 'GET',
      cache: 'no-store',
      credentials: 'omit',
      signal: controller.signal,
    });
  } catch (error) {
    const timedOut = error instanceof Error && error.name === 'AbortError';
    return fail(timedOut ? 'timed out after 800 ms' : 'could not connect');
  } finally {
    clearTimeout(timer);
  }

  if (!response || !response.ok) {
    return fail(response && response.status ? 'HTTP ' + response.status : 'the request failed');
  }

  let body: Record<string, unknown>;
  try {
    body = (await response.json()) as Record<string, unknown>;
  } catch {
    return fail('response was not JSON');
  }

  const browser = typeof body.Browser === 'string' ? body.Browser.trim() : '';
  const webSocketDebuggerUrl = typeof body.webSocketDebuggerUrl === 'string' ? body.webSocketDebuggerUrl.trim() : '';
  if (!browser || !webSocketDebuggerUrl) return fail('missing Browser or webSocketDebuggerUrl');

  const protocolVersion =
    typeof body['Protocol-Version'] === 'string' ? (body['Protocol-Version'] as string).trim() : null;
  const browserFamily = browserFamilyFromProduct(browser);
  const comparison = compareBrowserFamily(expectedBrowserFamily, browserFamily);

  return {
    available: true,
    endpoint,
    webSocketDebuggerUrl,
    browser,
    browserFamily,
    expectedBrowserFamily,
    familyMatch: comparison.match,
    matchKind: comparison.kind,
    protocolVersion: protocolVersion || null,
    source,
    reason: null,
  };
}

export async function probeCdpEndpoint(endpoint?: string | null): Promise<CdpProbeResult> {
  const requested = endpoint === undefined || endpoint === null || endpoint === '' ? DEFAULT_CDP_ENDPOINT : endpoint;
  const expectedBrowserFamily = detectBrowserFamily();
  const validation = validateCdpEndpoint(requested);
  if (!validation.ok || !validation.value) {
    return unavailable(null, 'configured', validation.error || 'Invalid CDP endpoint', expectedBrowserFamily);
  }

  if (validation.value !== DEFAULT_CDP_ENDPOINT) {
    return probeOne(validation.value, 'configured', expectedBrowserFamily);
  }

  const results = await Promise.all(
    CANDIDATE_PORTS.map((port, index) => {
      return probeOne('http://127.0.0.1:' + String(port), 'auto', expectedBrowserFamily).then((item) =>
        Object.assign({ index }, item),
      );
    }),
  );

  const available = results.filter((item): item is CdpProbeAvailable & { index: number } => item.available === true);
  const matching = available
    .map((item) => ({ item, comparison: compareBrowserFamily(expectedBrowserFamily, item.browserFamily) }))
    .filter((entry) => entry.comparison.match === true)
    .sort((left, right) => {
      const rank = (entry: { comparison: { kind: 'exact' | 'chromium' | null } }) =>
        entry.comparison.kind === 'exact' ? 0 : 1;
      return rank(left) - rank(right) || left.item.index - right.item.index;
    });

  if (matching.length > 0) {
    const selected = matching[0];
    return Object.assign({}, selected.item, {
      expectedBrowserFamily,
      familyMatch: true,
      matchKind: selected.comparison.kind,
    });
  }

  if (available.length > 0 && expectedBrowserFamily === 'unknown') {
    return Object.assign({}, available[0], { expectedBrowserFamily, familyMatch: null, matchKind: null });
  }

  if (available.length > 0) {
    const candidates = available.map((item) => (item.browserFamily || 'unknown') + '@' + item.endpoint).join(', ');
    return unavailable(
      null,
      'auto',
      'no matching browser CDP endpoint (expected ' + expectedBrowserFamily + '; saw ' + candidates + ')',
      expectedBrowserFamily,
      false,
    );
  }

  return unavailable(null, 'auto', 'no CDP endpoint on ports ' + CANDIDATE_PORTS.join(', '), expectedBrowserFamily);
}

export async function detectCdpEndpoint(): Promise<CdpProbeResult> {
  const endpoint = await readCdpEndpoint();
  return probeCdpEndpoint(endpoint);
}
