'use strict';

/**
 * @fileoverview Shared CDP endpoint helper.
 *
 * The extension never attaches a debugger to detect a CDP endpoint. It asks a
 * loopback HTTP endpoint for /json/version, which is a read-only probe. The
 * service worker, options page and temporary harnesses use the same helper.
 *
 * Plain JavaScript, no build step, no dependencies.
 */

(() => {
  const DEFAULT_CDP_ENDPOINT = 'auto';
  const STORAGE_KEY = 'corvus.cdpEndpoint';
  const CANDIDATE_PORTS = Object.freeze([9222, 9223, 9333]);
  const PROBE_TIMEOUT_MS = 800;
  const PROBE_PATH = '/json/version';
  const MAX_REASON_LENGTH = 160;

  const CHROMIUM_BRANDS = Object.freeze(['chrome', 'chromium']);

  function currentNavigator() {
    if (typeof self !== 'undefined' && self && self.navigator) return self.navigator;
    if (typeof navigator !== 'undefined' && navigator) return navigator;
    return null;
  }

  function brandNames(nav) {
    const data = nav && nav.userAgentData;
    const brands = data && Array.isArray(data.brands) ? data.brands : [];
    return brands.map((item) => String(item && item.brand ? item.brand : '').toLowerCase());
  }

  /**
   * Derive the extension browser family from navigator.userAgentData or the
   * user agent. Returns chrome, chromium, edge, brave, opera, vivaldi or unknown.
   */
  function detectBrowserFamily(nav) {
    const target = nav || currentNavigator();
    const names = brandNames(target);
    const hasBrand = (needle) => names.some((name) => name.indexOf(needle) !== -1);

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

  /** Derive a family from the CDP Browser product string, such as Edg/157.0. */
  function browserFamilyFromProduct(product) {
    const value = String(product || '').trim().toLowerCase();
    if (!value) return 'unknown';
    if (value.indexOf('edg/') === 0 || value.indexOf('edge/') === 0 || value.indexOf('microsoft edge') === 0) return 'edge';
    if (value.indexOf('brave/') === 0) return 'brave';
    if (value.indexOf('opr/') === 0 || value.indexOf('opera/') === 0) return 'opera';
    if (value.indexOf('vivaldi/') === 0) return 'vivaldi';
    if (value.indexOf('headlesschrome/') === 0) return 'chrome';
    if (value.indexOf('chrome/') === 0 || value.indexOf('google chrome/') === 0) return 'chrome';
    if (value.indexOf('chromium/') === 0) return 'chromium';
    return 'unknown';
  }

  function isChromiumBrand(family) {
    return CHROMIUM_BRANDS.indexOf(family) !== -1;
  }

  /**
   * Compare the extension browser family with the endpoint family.
   * Chrome and Chromium are one Chromium brand family. Every branded Chromium
   * browser (Edge, Brave, Opera, Vivaldi) is a distinct family, so auto
   * detection never silently binds the wrong browser.
   */
  function compareBrowserFamily(expected, actual) {
    if (!expected || expected === 'unknown' || !actual || actual === 'unknown') {
      return { match: null, kind: null };
    }
    if (expected === actual) return { match: true, kind: 'exact' };
    if (isChromiumBrand(expected) && isChromiumBrand(actual)) return { match: true, kind: 'chromium' };
    return { match: false, kind: null };
  }

  function shorten(value) {
    if (typeof value !== 'string') return null;
    const text = value.replace(/\s+/g, ' ').trim();
    return text ? text.slice(0, MAX_REASON_LENGTH) : null;
  }

  function isLoopbackHostname(hostname) {
    if (typeof hostname !== 'string') return false;
    const host = hostname.toLowerCase();
    return host === '127.0.0.1' || host === 'localhost' || host === '[::1]' || host === '::1';
  }

  /**
   * Validate auto or an explicit HTTP loopback endpoint.
   *
   * An empty value means auto. Explicit endpoints must use http://, a loopback
   * host, an explicit numeric port, and no path, query string or credentials.
   */
  function validate(value) {
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

    let url;
    try {
      url = new URL(text);
    } catch (error) {
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

  /** Normalize a value, returning null when it is not acceptable. */
  function normalize(value) {
    const result = validate(value);
    return result.ok ? result.value : null;
  }

  function requireValid(value) {
    const result = validate(value);
    if (!result.ok) {
      const error = new Error(result.error);
      error.code = 'INVALID_CDP_ENDPOINT';
      throw error;
    }
    return result.value;
  }

  /** Return the stored endpoint, or auto when absent or invalid. */
  async function readCdpEndpoint() {
    if (typeof chrome === 'undefined' || !chrome.storage || !chrome.storage.local) {
      return DEFAULT_CDP_ENDPOINT;
    }
    try {
      const stored = await chrome.storage.local.get(STORAGE_KEY);
      return normalize(stored && stored[STORAGE_KEY]) || DEFAULT_CDP_ENDPOINT;
    } catch (error) {
      return DEFAULT_CDP_ENDPOINT;
    }
  }

  /** Persist a normalized endpoint and return it. Throws when validation or storage fails. */
  async function writeCdpEndpoint(value) {
    const normalized = requireValid(value);
    await chrome.storage.local.set({ [STORAGE_KEY]: normalized });
    return normalized;
  }

  /** Remove the stored override so detection falls back to auto. */
  async function resetCdpEndpoint() {
    await chrome.storage.local.remove(STORAGE_KEY);
    return DEFAULT_CDP_ENDPOINT;
  }

  function unavailable(endpoint, source, reason, expectedBrowserFamily, familyMatch) {
    return {
      available: false,
      endpoint: endpoint || null,
      webSocketDebuggerUrl: null,
      browser: null,
      browserFamily: null,
      expectedBrowserFamily: expectedBrowserFamily || null,
      familyMatch: typeof familyMatch === 'boolean' ? familyMatch : null,
      protocolVersion: null,
      source: source || DEFAULT_CDP_ENDPOINT,
      reason: shorten(reason) || 'CDP endpoint unavailable',
    };
  }

  async function probeOne(endpoint, source, expectedBrowserFamily) {
    const expected = expectedBrowserFamily || detectBrowserFamily();
    const fail = (reason, familyMatch) => unavailable(endpoint, source, reason, expected, familyMatch);

    if (typeof fetch !== 'function') {
      return fail('fetch is unavailable');
    }

    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), PROBE_TIMEOUT_MS);
    let response;
    try {
      response = await fetch(endpoint + PROBE_PATH, {
        method: 'GET',
        cache: 'no-store',
        credentials: 'omit',
        signal: controller.signal,
      });
    } catch (error) {
      const timedOut = error && error.name === 'AbortError';
      return fail(timedOut ? 'timed out after 800 ms' : 'could not connect');
    } finally {
      clearTimeout(timer);
    }

    if (!response || !response.ok) {
      const status = response && response.status ? 'HTTP ' + response.status : 'the request failed';
      return fail(status);
    }

    let body;
    try {
      body = await response.json();
    } catch (error) {
      return fail('response was not JSON');
    }

    const browser = body && typeof body.Browser === 'string' ? body.Browser.trim() : '';
    const webSocketDebuggerUrl = body && typeof body.webSocketDebuggerUrl === 'string'
      ? body.webSocketDebuggerUrl.trim()
      : '';
    if (!browser || !webSocketDebuggerUrl) {
      return fail('missing Browser or webSocketDebuggerUrl');
    }

    const protocolVersion = body && typeof body['Protocol-Version'] === 'string'
      ? body['Protocol-Version'].trim()
      : null;
    const browserFamily = browserFamilyFromProduct(browser);
    const comparison = compareBrowserFamily(expected, browserFamily);

    return {
      available: true,
      endpoint,
      webSocketDebuggerUrl,
      browser,
      browserFamily,
      expectedBrowserFamily: expected,
      familyMatch: comparison.match,
      matchKind: comparison.kind,
      protocolVersion: protocolVersion || null,
      source,
      reason: null,
    };
  }

  /**
   * Probe auto or one explicit endpoint. Never throws; failures are returned as
   * available:false with a short reason.
   */
  async function probe(endpoint) {
    const requested = endpoint === undefined || endpoint === null || endpoint === ''
      ? DEFAULT_CDP_ENDPOINT
      : endpoint;
    const expectedBrowserFamily = detectBrowserFamily();
    const validation = validate(requested);
    if (!validation.ok) {
      return unavailable(null, 'configured', validation.error, expectedBrowserFamily);
    }

    if (validation.value !== DEFAULT_CDP_ENDPOINT) {
      return probeOne(validation.value, 'configured', expectedBrowserFamily);
    }

    const results = await Promise.all(CANDIDATE_PORTS.map((port, index) => {
      return probeOne('http://127.0.0.1:' + String(port), DEFAULT_CDP_ENDPOINT, expectedBrowserFamily)
        .then((item) => Object.assign({ index }, item));
    }));

    const available = results.filter((item) => item.available === true);
    const matching = available
      .map((item) => ({ item, comparison: compareBrowserFamily(expectedBrowserFamily, item.browserFamily) }))
      .filter((entry) => entry.comparison.match === true)
      .sort((left, right) => {
        const rank = (entry) => (entry.comparison.kind === 'exact' ? 0 : 1);
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

    if (available.length > 0) {
      const candidates = available
        .map((item) => (item.browserFamily || 'unknown') + '@' + item.endpoint)
        .join(', ');
      const reason = 'no matching browser CDP endpoint (expected '
        + expectedBrowserFamily + '; saw ' + candidates + ')';
      return unavailable(null, DEFAULT_CDP_ENDPOINT, reason, expectedBrowserFamily, false);
    }

    return unavailable(null, DEFAULT_CDP_ENDPOINT, 'no CDP endpoint on ports ' + CANDIDATE_PORTS.join(', '), expectedBrowserFamily);
  }

  /** Read the configured endpoint and probe it. Never throws. */
  async function detect() {
    const endpoint = await readCdpEndpoint();
    return probe(endpoint);
  }

  const api = Object.freeze({
    DEFAULT_CDP_ENDPOINT,
    STORAGE_KEY,
    CANDIDATE_PORTS,
    PROBE_TIMEOUT_MS,
    detectBrowserFamily,
    browserFamilyFromProduct,
    validate,
    normalize,
    readCdpEndpoint,
    writeCdpEndpoint,
    resetCdpEndpoint,
    probe,
    detect,
  });

  const root = typeof self !== 'undefined' ? self : globalThis;
  root.CorvusCdpEndpoint = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
