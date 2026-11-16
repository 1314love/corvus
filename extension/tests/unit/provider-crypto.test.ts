import { createHash, createHmac } from 'node:crypto';
import { describe, expect, it } from 'vitest';
import { MAX_INNER_PLAINTEXT_BYTES, MAX_SEQUENCE, ProviderProtocolError } from '../../src/shared/provider/protocol';
import {
  computeSharedSecret,
  confirmationProof,
  createTranscript,
  decryptFrame,
  deriveSessionKeys,
  encryptFrame,
  exportPublicKeyRaw,
  frameAad,
  frameNonce,
  generateP256KeyPair,
  hkdfExpand,
  hkdfExtract,
  importAesGcmKey,
  pairingProof,
  randomBytes,
  randomUuid,
} from '../../src/shared/provider/crypto';

function hmacHex(key: Uint8Array, data: Uint8Array): string {
  return createHmac('sha256', Buffer.from(key)).update(Buffer.from(data)).digest('hex');
}

function hex(bytes: Uint8Array): string {
  return Buffer.from(bytes).toString('hex');
}

async function expectProtocolError(action: () => Promise<unknown>, closeCode = 4404): Promise<ProviderProtocolError> {
  try {
    await action();
  } catch (error) {
    expect(error).toBeInstanceOf(ProviderProtocolError);
    expect((error as ProviderProtocolError).closeCode).toBe(closeCode);
    return error as ProviderProtocolError;
  }
  throw new Error('expected a ProviderProtocolError');
}

describe('provider crypto', () => {
  it('implements the RFC 5869 SHA-256 HKDF test vector and the protocol key schedule', async () => {
    const ikm = new Uint8Array(22).fill(0x0b);
    const salt = Uint8Array.from(Array.from({ length: 13 }, (_, index) => index));
    const info = Uint8Array.from(Array.from({ length: 10 }, (_, index) => 0xf0 + index));
    const prk = await hkdfExtract(salt, ikm);
    const okm = await hkdfExpand(prk, info, 32);

    expect(hex(prk)).toBe(hmacHex(salt, ikm));
    expect(hex(okm)).toBe('3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf');

    const shared = randomBytes(32);
    const token = randomBytes(32);
    const keys = await deriveSessionKeys({ salt, sharedSecret: shared, tokenBytes: token });
    const tokenDigest = createHash('sha256')
      .update(Buffer.concat([Buffer.from('corvus-provider-v2-token'), Buffer.from(token)]))
      .digest();
    const expectedPrk = createHmac('sha256', Buffer.from(salt)).update(Buffer.from(shared)).digest();
    const labels = [
      ['c2s', 'c2s'],
      ['s2c', 's2c'],
      ['server-confirm', 'serverConfirm'],
      ['client-confirm', 'clientConfirm'],
    ] as const;
    for (const [label, key] of labels) {
      const expectedInfo = Buffer.concat([
        Buffer.from('corvus-provider-v2/'),
        Buffer.from(label),
        Buffer.from([0]),
        tokenDigest,
      ]);
      const expected = createHmac('sha256', expectedPrk)
        .update(Buffer.concat([expectedInfo, Buffer.from([1])]))
        .digest()
        .subarray(0, 32);
      expect(hex(keys[key])).toBe(expected.toString('hex'));
    }
  });

  it('builds the transcript, pairing proof and confirmation proofs from exact bytes', async () => {
    const serverNonce = Uint8Array.of(1, 2, 3);
    const clientNonce = Uint8Array.of(4, 5, 6);
    const serverEphemeral = Uint8Array.of(7, 8);
    const clientEphemeral = Uint8Array.of(9, 10);
    const clientId = Uint8Array.of(11, 12);
    const clientPublicKey = Uint8Array.of(13, 14);
    const transcript = await createTranscript({
      serverNonce,
      clientNonce,
      serverEphemeral,
      clientEphemeral,
      clientId,
      clientPublicKey,
    });
    const expectedTranscript = createHash('sha256')
      .update(
        Buffer.concat([
          Buffer.from('corvus-provider-v2-transcript\0'),
          Buffer.from(serverNonce),
          Buffer.from(clientNonce),
          Buffer.from(serverEphemeral),
          Buffer.from(clientEphemeral),
          Buffer.from(clientId),
          Buffer.from(clientPublicKey),
        ]),
      )
      .digest();
    expect(hex(transcript)).toBe(expectedTranscript.toString('hex'));

    const token = randomBytes(32);
    const proof = await pairingProof(token, transcript);
    expect(hex(proof)).toBe(hmacHex(token, transcript));

    const key = randomBytes(32);
    const confirmation = await confirmationProof(key, transcript);
    expect(hex(confirmation)).toBe(hmacHex(key, transcript));
  });

  it('derives the same ECDH secret from both sides', async () => {
    const first = await generateP256KeyPair();
    const second = await generateP256KeyPair();
    const firstPublic = await exportPublicKeyRaw(first.publicKey);
    const secondPublic = await exportPublicKeyRaw(second.publicKey);
    expect(firstPublic).toHaveLength(65);
    expect(secondPublic).toHaveLength(65);
    expect(firstPublic[0]).toBe(0x04);
    const firstSecret = await computeSharedSecret(first.privateKey, secondPublic);
    const secondSecret = await computeSharedSecret(second.privateKey, firstPublic);
    expect(hex(firstSecret)).toBe(hex(secondSecret));
    expect(firstSecret).toHaveLength(32);
  });

  it('constructs the exact frame nonce and AAD', () => {
    expect(hex(frameNonce(0x01020304))).toBe('000000000000000001020304');
    const expectedAad = Buffer.concat([
      Buffer.from('corvus-provider-v2-frame\0'),
      Buffer.from([1]),
      Buffer.from([0, 0, 0, 0, 1, 2, 3, 4]),
    ]);
    expect(hex(frameAad('c2s', 0x01020304))).toBe(expectedAad.toString('hex'));
    expect(hex(frameAad('s2c', 0x01020304))).toBe(
      Buffer.concat([
        Buffer.from('corvus-provider-v2-frame\0'),
        Buffer.from([2]),
        Buffer.from([0, 0, 0, 0, 1, 2, 3, 4]),
      ]).toString('hex'),
    );
  });

  it('accepts sequence numbers above 32 bits in the same 8-byte encoding', () => {
    const seq = 0x1_0002_0304;
    expect(seq).toBeLessThanOrEqual(MAX_SEQUENCE);
    expect(hex(frameNonce(seq))).toBe('000000000000000100020304');
    const expectedAad = Buffer.concat([
      Buffer.from('corvus-provider-v2-frame\0'),
      Buffer.from([1]),
      Buffer.from([0, 0, 0, 1, 0, 2, 3, 4]),
    ]);
    expect(frameAad('c2s', seq)).toHaveLength(34);
    expect(hex(frameAad('c2s', seq))).toBe(expectedAad.toString('hex'));
  });

  it('round-trips AES-256-GCM frames and rejects tampering, replay and direction confusion', async () => {
    const rawKey = randomBytes(32);
    const key = await importAesGcmKey(rawKey);
    const plaintext = new TextEncoder().encode('provider frame payload');
    const ciphertext = await encryptFrame(key, plaintext, 'c2s', 7);
    expect(ciphertext).toHaveLength(plaintext.length + 16);
    expect(new TextDecoder().decode(await decryptFrame(key, ciphertext, 'c2s', 7))).toBe('provider frame payload');

    const tampered = new Uint8Array(ciphertext);
    tampered[0] ^= 0xff;
    await expectProtocolError(() => decryptFrame(key, tampered, 'c2s', 7), 4404);
    await expectProtocolError(() => decryptFrame(key, ciphertext, 'c2s', 8), 4404);
    await expectProtocolError(() => decryptFrame(key, ciphertext, 's2c', 7), 4404);
    await expectProtocolError(() => decryptFrame(key, ciphertext.subarray(0, 15), 'c2s', 7), 4404);
  });

  it('rejects oversized plaintext before encryption', async () => {
    const key = await importAesGcmKey(randomBytes(32));
    const oversized = new Uint8Array(MAX_INNER_PLAINTEXT_BYTES + 1);
    const error = await expectProtocolError(() => encryptFrame(key, oversized, 'c2s', 0), 1013);
    expect(error.message).not.toContain('frame payload');
  });

  it('creates UUIDs and random material without exposing a token', async () => {
    expect(randomUuid()).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
    expect(randomBytes(32)).toHaveLength(32);
    await expectProtocolError(() => Promise.resolve(frameNonce(-1)), 4404);
    await expectProtocolError(() => Promise.resolve(frameNonce(1.5)), 4404);
    await expectProtocolError(() => Promise.resolve(frameNonce(MAX_SEQUENCE + 1)), 4404);
    await expectProtocolError(() => Promise.resolve(frameAad('c2s', MAX_SEQUENCE + 1)), 4404);
    expect(frameNonce(MAX_SEQUENCE)).toHaveLength(12);
  });
});
