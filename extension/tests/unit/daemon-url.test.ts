import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  DEFAULT_DAEMON_URL,
  DAEMON_URL_STORAGE_KEY,
  normalizeDaemonUrl,
  readDaemonUrl,
  resetDaemonUrl,
  validateDaemonUrl,
  writeDaemonUrl,
} from '../../src/shared/storage/daemon-url';

function stubStorage(initial: Record<string, unknown> = {}) {
  const data: Record<string, unknown> = { ...initial };
  const get = vi.fn(async (keys?: string | string[]) => {
    if (typeof keys === 'string') return { [keys]: data[keys] };
    return { ...data };
  });
  const set = vi.fn(async (items: Record<string, unknown>) => {
    Object.assign(data, items);
  });
  const remove = vi.fn(async (keys: string | string[]) => {
    for (const key of Array.isArray(keys) ? keys : [keys]) delete data[key];
  });
  vi.stubGlobal('chrome', { storage: { local: { get, set, remove } } });
  return { data, get, set, remove };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('daemon URL validation', () => {
  it('defaults to the loopback daemon', () => {
    expect(DEFAULT_DAEMON_URL).toBe('ws://127.0.0.1:7330');
  });

  it('accepts ws and wss hosts and normalizes one trailing slash', () => {
    expect(normalizeDaemonUrl(' ws://127.0.0.1:7330/ ')).toBe('ws://127.0.0.1:7330');
    expect(normalizeDaemonUrl('wss://bridge.example.test:443/')).toBe('wss://bridge.example.test');
    expect(validateDaemonUrl('ws://localhost:7330').ok).toBe(true);
  });

  it('rejects invalid schemes, paths, credentials and empty values', () => {
    for (const value of [
      '',
      'http://127.0.0.1:7330',
      'ws://127.0.0.1:7330/path',
      'ws://user:pass@127.0.0.1:7330',
      'ws://127.0.0.1:7330?x=1',
    ]) {
      expect(validateDaemonUrl(value).ok).toBe(false);
    }
  });

  it('reads the stored URL and falls back on bad or missing storage', async () => {
    stubStorage({ [DAEMON_URL_STORAGE_KEY]: 'ws://127.0.0.1:7444' });
    await expect(readDaemonUrl()).resolves.toBe('ws://127.0.0.1:7444');

    stubStorage({ [DAEMON_URL_STORAGE_KEY]: 'not-a-url' });
    await expect(readDaemonUrl()).resolves.toBe(DEFAULT_DAEMON_URL);
  });

  it('returns the default when chrome.storage is unavailable', async () => {
    vi.unstubAllGlobals();
    await expect(readDaemonUrl()).resolves.toBe(DEFAULT_DAEMON_URL);
  });

  it('writes normalized values and reset removes the key', async () => {
    const harness = stubStorage();
    await expect(writeDaemonUrl('ws://127.0.0.1:7555/')).resolves.toBe('ws://127.0.0.1:7555');
    expect(harness.data[DAEMON_URL_STORAGE_KEY]).toBe('ws://127.0.0.1:7555');

    await expect(resetDaemonUrl()).resolves.toBe(DEFAULT_DAEMON_URL);
    expect(harness.remove).toHaveBeenCalledWith(DAEMON_URL_STORAGE_KEY);
    expect(harness.data[DAEMON_URL_STORAGE_KEY]).toBeUndefined();
  });

  it('rejects invalid writes', async () => {
    stubStorage();
    await expect(writeDaemonUrl('http://127.0.0.1:7330')).rejects.toThrow('Use ws:// or wss://');
  });
});
