import { describe, expect, it } from 'vitest';
import {
  MAX_HANDSHAKE_FRAME_BYTES,
  MAX_SEQUENCE,
  ProviderProtocolError,
  decodeBase64Url,
  encodeBase64,
  encodeBase64Url,
  parseClientHello,
  parseFrame,
  parseFrameText,
  parseInvoke,
  parseManifest,
  parseProviderMessage,
  parseProviderMessageText,
  parseProviderTabs,
  parseResult,
  parseServerHello,
  parseServerHelloText,
  serializeMessage,
} from '../../src/shared/provider/protocol';

const UUID = '123e4567-e89b-42d3-a456-426614174000';
const SESSION_ID = 's-' + UUID;
const KEY_B64 = encodeBase64(Uint8Array.from([4, ...new Uint8Array(64).fill(7)]));
const NONCE_B64 = encodeBase64(new Uint8Array(32).fill(8));
const SALT_B64 = encodeBase64(new Uint8Array(32).fill(9));
const SERVER_ID = encodeBase64Url(new Uint8Array(16).fill(10));

function serverHello(): Record<string, unknown> {
  return {
    kind: 'server_hello',
    protocol: 2,
    server_id: SERVER_ID,
    server_nonce: NONCE_B64,
    server_ephemeral: KEY_B64,
    salt: SALT_B64,
    pairing_required: true,
  };
}

function clientHello(): Record<string, unknown> {
  return {
    kind: 'client_hello',
    protocol: 2,
    client_id: encodeBase64Url(new Uint8Array(16).fill(1)),
    client_public_key: KEY_B64,
    client_nonce: NONCE_B64,
    client_ephemeral: KEY_B64,
    pairing_proof: encodeBase64(new Uint8Array(32).fill(2)),
  };
}

function manifest(): Record<string, unknown> {
  return {
    kind: 'manifest',
    provider_id: 'provider-' + UUID,
    version: '2.0.0',
    browser: { family: 'Chrome', version: '157.0.0.0', profile: 'default' },
    capabilities: [
      { id: 'system.info', version: 1, actions: ['info'] },
      { id: 'tabs', version: 1, actions: ['list'] },
    ],
    subscriptions: ['tabs.updated'],
    limits: { max_invoke_bytes: 1048576, max_result_bytes: 8388608 },
  };
}

function expectProtocolError(action: () => unknown, closeCode = 4404): ProviderProtocolError {
  try {
    action();
  } catch (error) {
    expect(error).toBeInstanceOf(ProviderProtocolError);
    expect((error as ProviderProtocolError).closeCode).toBe(closeCode);
    return error as ProviderProtocolError;
  }
  throw new Error('expected a ProviderProtocolError');
}

describe('provider protocol validators', () => {
  it('accepts valid hello messages and rejects unknown or malformed fields', () => {
    expect(parseServerHello(serverHello()).kind).toBe('server_hello');
    expect(parseClientHello(clientHello()).kind).toBe('client_hello');
    expect(() => parseServerHello({ ...serverHello(), extra: true })).toThrow(ProviderProtocolError);
    expect(() => parseServerHello({ ...serverHello(), server_ephemeral: encodeBase64(new Uint8Array(65)) })).toThrow(
      ProviderProtocolError,
    );
    expect(() => parseClientHello({ ...clientHello(), client_nonce: encodeBase64(new Uint8Array(31)) })).toThrow(
      ProviderProtocolError,
    );
    expect(() => parseServerHello({ ...serverHello(), protocol: 0 })).toThrow(ProviderProtocolError);
  });

  it('rejects malformed and oversize JSON text with the frozen close codes', () => {
    expect(expectProtocolError(() => parseServerHelloText('{')).closeCode).toBe(4404);
    const tooLarge = JSON.stringify(serverHello()) + ' '.repeat(MAX_HANDSHAKE_FRAME_BYTES);
    expect(expectProtocolError(() => parseServerHelloText(tooLarge), 1013).closeCode).toBe(1013);
    expect(expectProtocolError(() => parseProviderMessageText('[]')).closeCode).toBe(4404);
  });

  it('validates encrypted frames strictly', () => {
    const valid = { seq: 0, ciphertext: encodeBase64(new Uint8Array(16).fill(3)) };
    expect(MAX_SEQUENCE).toBe(Number.MAX_SAFE_INTEGER);
    expect(parseFrame(valid)).toEqual(valid);
    expect(() => parseFrame({ ...valid, extra: 1 })).toThrow(ProviderProtocolError);
    expect(() => parseFrame({ ...valid, seq: -1 })).toThrow(ProviderProtocolError);
    expect(parseFrame({ ...valid, seq: 0x100000000 }).seq).toBe(0x100000000);
    expect(parseFrame({ ...valid, seq: MAX_SEQUENCE }).seq).toBe(MAX_SEQUENCE);
    expect(() => parseFrame({ ...valid, seq: 1.5 })).toThrow(ProviderProtocolError);
    expect(() => parseFrame({ ...valid, seq: MAX_SEQUENCE + 1 })).toThrow(ProviderProtocolError);
    expect(() => parseFrame({ ...valid, ciphertext: encodeBase64(new Uint8Array(15)) })).toThrow(ProviderProtocolError);
    expect(expectProtocolError(() => parseFrameText('{')).closeCode).toBe(4404);
  });

  it('enforces the backend manifest contract', () => {
    expect(parseManifest(manifest()).version).toBe('2.0.0');
    expect(() => parseManifest({ ...manifest(), version: '2.0.1' })).toThrow(ProviderProtocolError);
    expect(() =>
      parseManifest({
        ...manifest(),
        browser: { family: 'Chrome-1', version: '157', profile: 'default' },
      }),
    ).toThrow(ProviderProtocolError);
    expect(() =>
      parseManifest({
        ...manifest(),
        browser: { family: 'Chrome', version: '157', profile: 'x'.repeat(65) },
      }),
    ).toThrow(ProviderProtocolError);
    expect(() =>
      parseManifest({
        ...manifest(),
        capabilities: [{ id: 'system.info', version: 1, actions: ['info.bad'] }],
      }),
    ).toThrow(ProviderProtocolError);
    expect(() => parseManifest({ ...manifest(), subscriptions: ['other.topic'] })).toThrow(ProviderProtocolError);
    expect(() =>
      parseManifest({
        ...manifest(),
        limits: { max_invoke_bytes: 16 * 1024 * 1024 + 1, max_result_bytes: 8388608 },
      }),
    ).toThrow(ProviderProtocolError);
  });

  it('rejects invoke and result shapes that a gateway could not execute', () => {
    const invoke = {
      kind: 'invoke',
      id: UUID,
      action: 'system.info.info',
      args: {},
      deadline_ms: 1000,
    };
    expect(parseInvoke(invoke).action).toBe('system.info.info');
    expect(() => parseInvoke({ ...invoke, extra: true })).toThrow(ProviderProtocolError);
    expect(() => parseInvoke({ ...invoke, deadline_ms: 0 })).toThrow(ProviderProtocolError);
    expect(() => parseResult({ kind: 'result', id: UUID, ok: true })).toThrow(ProviderProtocolError);
    expect(() =>
      parseResult({
        kind: 'result',
        id: UUID,
        ok: false,
        error: { code: 'bad_args', message: 'nope', retryable: false },
      }),
    ).toThrow(ProviderProtocolError);
  });

  it('round-trips messages and validates typed provider results', () => {
    const ping = { kind: 'ping' as const, nonce: 'abc' };
    expect(serializeMessage(ping)).toBe(JSON.stringify(ping));
    expect(parseProviderMessage(ping)).toEqual(ping);
    expect(() => parseProviderMessage({ kind: 'not_a_message' })).toThrow(ProviderProtocolError);

    const tabs = [{ tab_id: '1', window_id: 1, title: 'One', url: 'https://example.com/', active: true }];
    expect(parseProviderTabs(tabs)).toEqual(tabs);
    const tooMany = Array.from({ length: 201 }, (_, index) => ({
      tab_id: String(index),
      window_id: 1,
      title: 't',
      url: 'https://example.com/',
      active: false,
    }));
    expect(() => parseProviderTabs(tooMany)).toThrow(ProviderProtocolError);
  });

  it('rejects invalid base64url client ids and exposes the session id grammar', () => {
    expect(decodeBase64Url(encodeBase64Url(new Uint8Array(16))).length).toBe(16);
    expect(() => decodeBase64Url('***')).toThrow(ProviderProtocolError);
    expect(() => parseClientHello({ ...clientHello(), client_id: '***' })).toThrow(ProviderProtocolError);
    expect(() => parseClientHello({ ...clientHello(), client_id: encodeBase64Url(new Uint8Array(15)) })).toThrow(
      ProviderProtocolError,
    );
    expect(SESSION_ID.startsWith('s-')).toBe(true);
  });
});
