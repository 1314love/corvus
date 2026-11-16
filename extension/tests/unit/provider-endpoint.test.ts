import { describe, expect, it } from 'vitest';
import {
  DAEMON_URL_STORAGE_KEY,
  PAIRING_TOKEN_STORAGE_KEY,
  PROVIDER_ENDPOINT_PATH,
  isLoopbackHost,
  providerBaseFromStorageValue,
  providerEndpointFromBase,
} from '../../src/shared/provider/endpoint';
import { PAIRING_TOKEN_BYTES, normalizePairingToken } from '../../src/shared/provider/token';

const token = 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA';

describe('provider endpoint discovery', () => {
  it('accepts only loopback ws bases and appends the frozen provider path', () => {
    expect(PROVIDER_ENDPOINT_PATH).toBe('/provider/v2');
    expect(providerEndpointFromBase('ws://127.0.0.1:8123')).toBe('ws://127.0.0.1:8123/provider/v2');
    expect(providerEndpointFromBase('ws://localhost:8123/')).toBe('ws://localhost:8123/provider/v2');
    expect(providerEndpointFromBase('ws://[::1]:8123')).toBe('ws://[::1]:8123/provider/v2');
    expect(providerEndpointFromBase(' ws://127.0.0.1:8123 ')).toBe('ws://127.0.0.1:8123/provider/v2');
  });

  it('rejects remote, TLS, path, query, credential and non-websocket bases', () => {
    const rejected = [
      'https://127.0.0.1:8123',
      'wss://127.0.0.1:8123',
      'ws://10.0.0.5:8123',
      'ws://example.com:8123',
      'ws://user:pass@127.0.0.1:8123',
      'ws://127.0.0.1:8123/custom',
      'ws://127.0.0.1:8123/?x=1',
      'ws://127.0.0.1:8123/#x',
      'not a url',
      '',
      null,
    ];
    for (const base of rejected) expect(providerEndpointFromBase(base)).toBeNull();
  });

  it('classifies loopback hosts and falls back to the stored base', () => {
    expect(isLoopbackHost('127.0.0.1')).toBe(true);
    expect(isLoopbackHost('LOCALHOST')).toBe(true);
    expect(isLoopbackHost('::1')).toBe(true);
    expect(isLoopbackHost('0.0.0.0')).toBe(false);
    expect(providerBaseFromStorageValue('ws://localhost:9000')).toBe('ws://localhost:9000');
    expect(providerBaseFromStorageValue('https://example.com')).toBe('ws://127.0.0.1:7330');
    expect(PAIRING_TOKEN_STORAGE_KEY).toBe('provider.pairingToken');
    expect(DAEMON_URL_STORAGE_KEY).toBe('corvus.daemonUrl');
  });
});

describe('pairing token normalization', () => {
  it('requires 43 base64url characters decoding to 32 bytes', () => {
    expect(token).toHaveLength(43);
    expect(normalizePairingToken(token)).toBe(token);
    expect(PAIRING_TOKEN_BYTES).toBe(32);
    expect(normalizePairingToken(token + 'A')).toBeNull();
    expect(normalizePairingToken(token.slice(0, 42))).toBeNull();
    expect(normalizePairingToken('!'.repeat(43))).toBeNull();
    expect(normalizePairingToken('')).toBeNull();
    expect(normalizePairingToken(undefined)).toBeNull();
  });
});
