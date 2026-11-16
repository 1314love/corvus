/**
 * Corvus v2 provider cryptography (P1).
 *
 * WebCrypto only: P-256 ECDH, HKDF-SHA256, HMAC-SHA256 and AES-256-GCM.
 * The derivation and transcript byte layouts are part of the frozen protocol
 * contract and are covered by deterministic unit tests.
 */
import { MAX_INNER_PLAINTEXT_BYTES, MAX_SEQUENCE, ProviderProtocolError } from './protocol';

export type ProviderDirection = 'c2s' | 's2c';

export interface ProviderSessionKeys {
  c2s: Uint8Array;
  s2c: Uint8Array;
  serverConfirm: Uint8Array;
  clientConfirm: Uint8Array;
}

const encoder = new TextEncoder();
const TRANSCRIPT_PREFIX = encoder.encode('corvus-provider-v2-transcript\0');
const TOKEN_PREFIX = encoder.encode('corvus-provider-v2-token');
const KEY_PREFIX = encoder.encode('corvus-provider-v2/');
const FRAME_PREFIX = encoder.encode('corvus-provider-v2-frame\0');

function asBufferSource(bytes: Uint8Array): BufferSource {
  return bytes as unknown as BufferSource;
}

function toBytes(value: ArrayBuffer): Uint8Array {
  return new Uint8Array(value);
}

export function concatBytes(...parts: Uint8Array[]): Uint8Array {
  let total = 0;
  for (const part of parts) total += part.length;
  const out = new Uint8Array(total);
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}

export function randomBytes(length: number): Uint8Array {
  if (!Number.isInteger(length) || length < 0) throw new ProviderProtocolError('invalid random byte length');
  const out = new Uint8Array(length);
  crypto.getRandomValues(out);
  return out;
}

export function randomBase64Url(length: number): string {
  const bytes = randomBytes(length);
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

export function randomUuid(): string {
  const bytes = randomBytes(16);
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  let hex = '';
  for (const byte of bytes) hex += byte.toString(16).padStart(2, '0');
  return (
    hex.slice(0, 8) + '-' + hex.slice(8, 12) + '-' + hex.slice(12, 16) + '-' + hex.slice(16, 20) + '-' + hex.slice(20)
  );
}

export async function sha256(data: Uint8Array): Promise<Uint8Array> {
  return toBytes(await crypto.subtle.digest('SHA-256', asBufferSource(data)));
}

export async function hmacSha256(keyBytes: Uint8Array, data: Uint8Array): Promise<Uint8Array> {
  const key = await crypto.subtle.importKey('raw', asBufferSource(keyBytes), { name: 'HMAC', hash: 'SHA-256' }, false, [
    'sign',
  ]);
  return toBytes(await crypto.subtle.sign('HMAC', key, asBufferSource(data)));
}

export async function hmacSha256Verify(
  keyBytes: Uint8Array,
  data: Uint8Array,
  signature: Uint8Array,
): Promise<boolean> {
  const key = await crypto.subtle.importKey('raw', asBufferSource(keyBytes), { name: 'HMAC', hash: 'SHA-256' }, false, [
    'verify',
  ]);
  return crypto.subtle.verify('HMAC', key, asBufferSource(signature), asBufferSource(data));
}

export async function hkdfExtract(salt: Uint8Array, inputKeyMaterial: Uint8Array): Promise<Uint8Array> {
  return hmacSha256(salt, inputKeyMaterial);
}

export async function hkdfExpand(prk: Uint8Array, info: Uint8Array, length = 32): Promise<Uint8Array> {
  if (!Number.isInteger(length) || length < 1 || length > 32) {
    throw new ProviderProtocolError('invalid HKDF output length');
  }
  const blocks = Math.ceil(length / 32);
  let previous: Uint8Array = new Uint8Array(0);
  let output: Uint8Array = new Uint8Array(0);
  for (let block = 1; block <= blocks; block += 1) {
    previous = await hmacSha256(prk, concatBytes(previous, info, Uint8Array.of(block)));
    output = concatBytes(output, previous);
  }
  return output.slice(0, length);
}

export async function tokenHash(tokenBytes: Uint8Array): Promise<Uint8Array> {
  return sha256(concatBytes(TOKEN_PREFIX, tokenBytes));
}

export function keyInfo(label: string, tokenHashBytes: Uint8Array): Uint8Array {
  return concatBytes(KEY_PREFIX, encoder.encode(label), Uint8Array.of(0), tokenHashBytes);
}

export async function deriveSessionKeys(params: {
  salt: Uint8Array;
  sharedSecret: Uint8Array;
  tokenBytes: Uint8Array;
}): Promise<ProviderSessionKeys> {
  const tokenHashBytes = await tokenHash(params.tokenBytes);
  const prk = await hkdfExtract(params.salt, params.sharedSecret);
  return {
    c2s: await hkdfExpand(prk, keyInfo('c2s', tokenHashBytes)),
    s2c: await hkdfExpand(prk, keyInfo('s2c', tokenHashBytes)),
    serverConfirm: await hkdfExpand(prk, keyInfo('server-confirm', tokenHashBytes)),
    clientConfirm: await hkdfExpand(prk, keyInfo('client-confirm', tokenHashBytes)),
  };
}

export function generateP256KeyPair(): Promise<CryptoKeyPair> {
  return crypto.subtle.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, true, [
    'deriveBits',
  ]) as Promise<CryptoKeyPair>;
}

export async function exportPublicKeyRaw(publicKey: CryptoKey): Promise<Uint8Array> {
  return toBytes(await crypto.subtle.exportKey('raw', publicKey));
}

export async function importPeerPublicKey(raw: Uint8Array): Promise<CryptoKey> {
  if (raw.length !== 65 || raw[0] !== 0x04) throw new ProviderProtocolError('invalid P-256 public key');
  return crypto.subtle.importKey('raw', asBufferSource(raw), { name: 'ECDH', namedCurve: 'P-256' }, false, []);
}

export async function computeSharedSecret(privateKey: CryptoKey, peerPublicKey: Uint8Array): Promise<Uint8Array> {
  const peer = await importPeerPublicKey(peerPublicKey);
  return toBytes(await crypto.subtle.deriveBits({ name: 'ECDH', public: peer }, privateKey, 256));
}

export async function createTranscript(params: {
  serverNonce: Uint8Array;
  clientNonce: Uint8Array;
  serverEphemeral: Uint8Array;
  clientEphemeral: Uint8Array;
  clientId: Uint8Array;
  clientPublicKey: Uint8Array;
}): Promise<Uint8Array> {
  return sha256(
    concatBytes(
      TRANSCRIPT_PREFIX,
      params.serverNonce,
      params.clientNonce,
      params.serverEphemeral,
      params.clientEphemeral,
      params.clientId,
      params.clientPublicKey,
    ),
  );
}

export function pairingProof(tokenBytes: Uint8Array, transcript: Uint8Array): Promise<Uint8Array> {
  return hmacSha256(tokenBytes, transcript);
}

export function verifyPairingProof(
  tokenBytes: Uint8Array,
  transcript: Uint8Array,
  proof: Uint8Array,
): Promise<boolean> {
  return hmacSha256Verify(tokenBytes, transcript, proof);
}

export function confirmationProof(keyBytes: Uint8Array, transcript: Uint8Array): Promise<Uint8Array> {
  return hmacSha256(keyBytes, transcript);
}

export function verifyConfirmationProof(
  keyBytes: Uint8Array,
  transcript: Uint8Array,
  proof: Uint8Array,
): Promise<boolean> {
  return hmacSha256Verify(keyBytes, transcript, proof);
}

export function frameSequenceBytes(seq: number): Uint8Array {
  if (!Number.isInteger(seq) || seq < 0 || seq > MAX_SEQUENCE) {
    throw new ProviderProtocolError('invalid frame sequence');
  }
  const out = new Uint8Array(8);
  const view = new DataView(out.buffer);
  view.setUint32(0, Math.floor(seq / 0x100000000), false);
  view.setUint32(4, seq >>> 0, false);
  return out;
}

export function frameNonce(seq: number): Uint8Array {
  return concatBytes(new Uint8Array(4), frameSequenceBytes(seq));
}

export function frameAad(direction: ProviderDirection, seq: number): Uint8Array {
  if (direction !== 'c2s' && direction !== 's2c') throw new ProviderProtocolError('invalid frame direction');
  return concatBytes(FRAME_PREFIX, Uint8Array.of(direction === 'c2s' ? 1 : 2), frameSequenceBytes(seq));
}

export function importAesGcmKey(raw: Uint8Array): Promise<CryptoKey> {
  if (raw.length !== 32) throw new ProviderProtocolError('invalid AES-256 key length');
  return crypto.subtle.importKey('raw', asBufferSource(raw), { name: 'AES-GCM' }, false, ['encrypt', 'decrypt']);
}

export function importHmacKey(raw: Uint8Array): Promise<CryptoKey> {
  return crypto.subtle.importKey('raw', asBufferSource(raw), { name: 'HMAC', hash: 'SHA-256' }, false, [
    'sign',
    'verify',
  ]);
}

export async function encryptFrame(
  key: CryptoKey,
  plaintext: Uint8Array,
  direction: ProviderDirection,
  seq: number,
): Promise<Uint8Array> {
  if (plaintext.length > MAX_INNER_PLAINTEXT_BYTES) {
    throw new ProviderProtocolError('frame plaintext is too large', 1013);
  }
  const encrypted = await crypto.subtle.encrypt(
    {
      name: 'AES-GCM',
      iv: asBufferSource(frameNonce(seq)),
      additionalData: asBufferSource(frameAad(direction, seq)),
      tagLength: 128,
    },
    key,
    asBufferSource(plaintext),
  );
  return toBytes(encrypted);
}

export async function decryptFrame(
  key: CryptoKey,
  ciphertext: Uint8Array,
  direction: ProviderDirection,
  seq: number,
): Promise<Uint8Array> {
  if (ciphertext.length < 16) throw new ProviderProtocolError('invalid encrypted frame length', 4404);
  let decrypted: ArrayBuffer;
  try {
    decrypted = await crypto.subtle.decrypt(
      {
        name: 'AES-GCM',
        iv: asBufferSource(frameNonce(seq)),
        additionalData: asBufferSource(frameAad(direction, seq)),
        tagLength: 128,
      },
      key,
      asBufferSource(ciphertext),
    );
  } catch {
    throw new ProviderProtocolError('frame authentication failed', 4404);
  }
  if (decrypted.byteLength > MAX_INNER_PLAINTEXT_BYTES) {
    throw new ProviderProtocolError('frame plaintext is too large', 1013);
  }
  return toBytes(decrypted);
}
