import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  DEFAULT_CDP_ENDPOINT,
  CDP_ENDPOINT_STORAGE_KEY,
  compareBrowserFamily,
  detectCdpEndpoint,
  normalizeCdpEndpoint,
  probeCdpEndpoint,
  readCdpEndpoint,
  validateCdpEndpoint,
} from '../../src/shared/cdp-endpoint';

function okBody(browser = 'Chrome/157.0.0.0') {
  return {
    Browser: browser,
    webSocketDebuggerUrl: 'ws://127.0.0.1:9222/devtools/browser/abc',
    'Protocol-Version': '1.3',
  };
}

function jsonResponse(body: Record<string, unknown>, status = 200) {
  return { ok: status >= 200 && status < 300, status, json: async () => body } as Response;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('CDP endpoint validation', () => {
  it('accepts auto and loopback http endpoints', () => {
    expect(DEFAULT_CDP_ENDPOINT).toBe('auto');
    for (const value of ['auto', 'http://127.0.0.1:9222', 'http://localhost:9333/', 'http://[::1]:9222']) {
      expect(validateCdpEndpoint(value).ok, value).toBe(true);
    }
  });

  it('rejects remote hosts, https, paths and missing ports', () => {
    for (const value of [
      'http://example.test:9222',
      'https://127.0.0.1:9222',
      'http://127.0.0.1:9222/json',
      'http://127.0.0.1',
      'auto/',
    ]) {
      expect(validateCdpEndpoint(value).ok, value).toBe(false);
    }
    expect(normalizeCdpEndpoint(' http://127.0.0.1:9222/ ')).toBe('http://127.0.0.1:9222');
  });

  it('classifies browser families and exact versus chromium matches', () => {
    expect(compareBrowserFamily('chrome', 'chromium')).toEqual({ match: true, kind: 'chromium' });
    expect(compareBrowserFamily('chrome', 'chrome')).toEqual({ match: true, kind: 'exact' });
    expect(compareBrowserFamily('edge', 'chrome')).toEqual({ match: false, kind: null });
  });
});

describe('CDP probing', () => {
  it('probes auto ports and returns the first available endpoint', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.indexOf('9333') !== -1) return jsonResponse(okBody());
      return jsonResponse({}, 404);
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await probeCdpEndpoint('auto');
    expect(result.available).toBe(true);
    if (result.available) {
      expect(result.endpoint).toBe('http://127.0.0.1:9333');
      expect(result.protocolVersion).toBe('1.3');
      expect(result.source).toBe('auto');
    }
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });

  it('probes a configured endpoint only and validates the payload', async () => {
    const fetchMock = vi.fn(async (_input: RequestInfo | URL) => jsonResponse(okBody()));
    vi.stubGlobal('fetch', fetchMock);

    const result = await probeCdpEndpoint('http://127.0.0.1:9444');
    expect(result.available).toBe(true);
    if (result.available) expect(result.source).toBe('configured');
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(String(fetchMock.mock.calls[0][0])).toBe('http://127.0.0.1:9444/json/version');
  });

  it('returns unavailable with a short reason instead of throwing', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        throw new Error('boom');
      }),
    );
    const result = await probeCdpEndpoint('auto');
    expect(result.available).toBe(false);
    if (!result.available) expect(result.reason).toContain('no CDP endpoint on ports');
  });

  it('rejects an invalid configured endpoint without fetching', async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal('fetch', fetchMock);
    const result = await probeCdpEndpoint('http://example.test:9222');
    expect(result.available).toBe(false);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('reads the stored endpoint and falls back to auto', async () => {
    const data: Record<string, unknown> = { [CDP_ENDPOINT_STORAGE_KEY]: 'http://127.0.0.1:9223' };
    vi.stubGlobal('chrome', {
      storage: {
        local: {
          get: async (key: string) => ({ [key]: data[key] }),
          set: async () => undefined,
          remove: async () => undefined,
        },
      },
    });
    await expect(readCdpEndpoint()).resolves.toBe('http://127.0.0.1:9223');

    vi.stubGlobal('chrome', {
      storage: { local: { get: async () => ({}), set: async () => undefined, remove: async () => undefined } },
    });
    await expect(readCdpEndpoint()).resolves.toBe(DEFAULT_CDP_ENDPOINT);

    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        throw new Error('offline');
      }),
    );
    const result = await detectCdpEndpoint();
    expect(result.source).toBe('auto');
    expect(result.available).toBe(false);
  });
});
