// QA-only P2 provider harness. Speaks the frozen encrypted provider protocol.
import { readFileSync } from 'node:fs';
import { randomUUID, randomBytes } from 'node:crypto';

const subtle = globalThis.crypto.subtle;
const enc = new TextEncoder();
const dec = new TextDecoder();
const TRANSCRIPT_LABEL = enc.encode('corvus-provider-v2-transcript\0');
const FRAME_LABEL = enc.encode('corvus-provider-v2-frame\0');
const TOKEN_LABEL = enc.encode('corvus-provider-v2-token');
const KEY_LABEL = enc.encode('corvus-provider-v2/');

function b64(value) { return Buffer.from(value).toString('base64'); }
function b64u(value) { return Buffer.from(value).toString('base64url'); }
function fromB64(value) { return new Uint8Array(Buffer.from(value, 'base64')); }
function fromB64u(value) { return new Uint8Array(Buffer.from(value, 'base64url')); }
function concat(...parts) {
  const chunks = parts.map((part) => typeof part === 'string' ? enc.encode(part) : new Uint8Array(part));
  const total = chunks.reduce((sum, part) => sum + part.length, 0);
  const out = new Uint8Array(total);
  let offset = 0;
  for (const part of chunks) { out.set(part, offset); offset += part.length; }
  return out;
}
function u64(value) {
  const out = new Uint8Array(8);
  new DataView(out.buffer).setBigUint64(0, BigInt(value));
  return out;
}
const sha256 = async (data) => new Uint8Array(await subtle.digest('SHA-256', data));
async function hmac(key, data) {
  const imported = await subtle.importKey('raw', key, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  return new Uint8Array(await subtle.sign('HMAC', imported, data));
}
async function hkdf(shared, salt, info) {
  const imported = await subtle.importKey('raw', shared, 'HKDF', false, ['deriveBits']);
  return new Uint8Array(await subtle.deriveBits({ name: 'HKDF', hash: 'SHA-256', salt, info }, imported, 256));
}
async function genKey() {
  const pair = await subtle.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, true, ['deriveBits']);
  return { pair, publicKey: new Uint8Array(await subtle.exportKey('raw', pair.publicKey)) };
}
async function agree(privateKey, peer) {
  const imported = await subtle.importKey('raw', peer, { name: 'ECDH', namedCurve: 'P-256' }, false, []);
  return new Uint8Array(await subtle.deriveBits({ name: 'ECDH', public: imported }, privateKey, 256));
}
const aesKey = (raw) => subtle.importKey('raw', raw, { name: 'AES-GCM' }, false, ['encrypt', 'decrypt']);
function nonceFor(seq) { const nonce = new Uint8Array(12); nonce.set(u64(seq), 4); return nonce; }
function aadFor(direction, seq) { return concat(FRAME_LABEL, new Uint8Array([direction]), u64(seq)); }
async function seal(value, key, seq, direction) {
  const ciphertext = await subtle.encrypt({ name: 'AES-GCM', iv: nonceFor(seq), additionalData: aadFor(direction, seq) }, await aesKey(key), enc.encode(JSON.stringify(value)));
  return { seq, ciphertext: b64(new Uint8Array(ciphertext)) };
}
async function open(ciphertext, key, seq, direction) {
  const plaintext = await subtle.decrypt({ name: 'AES-GCM', iv: nonceFor(seq), additionalData: aadFor(direction, seq) }, await aesKey(key), fromB64(ciphertext));
  return JSON.parse(dec.decode(plaintext));
}

export function loadToken(file) {
  const token = readFileSync(file, 'utf8').trim();
  const bytes = fromB64u(token);
  if (bytes.length !== 32) throw new Error('pairing token must decode to 32 bytes');
  return { text: token, bytes };
}

export class ProviderConnection {
  constructor(ws) {
    this.ws = ws;
    this.events = [];
    this.waiters = [];
    this.closed = null;
    this.errors = [];
    ws.addEventListener('message', (event) => { this.events.push({ kind: 'message', data: event.data }); this.pump(); });
    ws.addEventListener('close', (event) => { this.closed = { code: event.code, reason: event.reason || '' }; this.pump(); });
    ws.addEventListener('error', () => { this.errors.push('socket error'); });
  }
  pump() {
    while (this.waiters.length > 0 && (this.events.length > 0 || this.closed)) {
      const waiter = this.waiters.shift();
      if (this.events.length > 0) waiter.resolve(this.events.shift());
      else waiter.resolve({ kind: 'close', ...this.closed });
    }
  }
  next(timeoutMs = 10000) {
    if (this.events.length > 0) return Promise.resolve(this.events.shift());
    if (this.closed) return Promise.resolve({ kind: 'close', ...this.closed });
    return new Promise((resolve, reject) => {
      const waiter = { resolve, reject };
      const timer = setTimeout(() => {
        this.waiters = this.waiters.filter((item) => item !== waiter);
        reject(new Error('timeout waiting for provider socket event'));
      }, timeoutMs);
      waiter.resolve = (value) => { clearTimeout(timer); resolve(value); };
      this.waiters.push(waiter);
    });
  }
  close() { try { this.ws.close(1000, 'qa done'); } catch {} }
}

export async function connectProvider(options) {
  const {
    url,
    token,
    providerId = 'provider-' + randomUUID(),
    browser = { family: 'chrome', version: '157.0.8103.78', profile: 'qa' },
    capabilities = [
      { id: 'tabs', version: 1, actions: ['list', 'open', 'close'] },
      { id: 'page', version: 1, actions: ['navigate', 'read', 'snapshot'] },
      { id: 'input', version: 1, actions: ['click', 'type'] },
      { id: 'capture', version: 1, actions: ['viewport'] },
    ],
    onInvoke,
    limits = { max_invoke_bytes: 1048576, max_result_bytes: 8388608 },
  } = options;
  const ws = new WebSocket(url);
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('provider websocket open timeout')), 5000);
    ws.addEventListener('open', () => { clearTimeout(timer); resolve(); }, { once: true });
    ws.addEventListener('error', () => { clearTimeout(timer); reject(new Error('provider websocket open failed')); }, { once: true });
  });
  const channel = new ProviderConnection(ws);
  const first = await channel.next(5000);
  if (first.kind !== 'message') throw new Error('provider closed before server_hello');
  const hello = JSON.parse(first.data);
  if (hello.kind !== 'server_hello' || hello.protocol !== 2) throw new Error('invalid server_hello');
  const identity = await genKey();
  const ephemeral = await genKey();
  const clientId = b64u(randomBytes(24));
  const clientNonce = randomBytes(32);
  const transcript = await sha256(concat(TRANSCRIPT_LABEL, fromB64(hello.server_nonce), clientNonce, fromB64(hello.server_ephemeral), ephemeral.publicKey, fromB64u(clientId), identity.publicKey));
  const proof = await hmac(token.bytes, transcript);
  ws.send(JSON.stringify({ kind: 'client_hello', protocol: 2, client_id: clientId, client_public_key: b64(identity.publicKey), client_nonce: b64(clientNonce), client_ephemeral: b64(ephemeral.publicKey), pairing_proof: b64(proof) }));
  const shared = await agree(ephemeral.pair.privateKey, fromB64(hello.server_ephemeral));
  const tokenHash = await sha256(concat(TOKEN_LABEL, token.bytes));
  const salt = fromB64(hello.salt);
  const derive = async (label) => hkdf(shared, salt, concat(KEY_LABEL, enc.encode(label), new Uint8Array([0]), tokenHash));
  const keys = { c2s: await derive('c2s'), s2c: await derive('s2c'), serverConfirm: await derive('server-confirm'), clientConfirm: await derive('client-confirm') };
  const scFrame = await channel.next(10000);
  if (scFrame.kind !== 'message') throw new Error('provider closed before server_confirm');
  const scEnvelope = JSON.parse(scFrame.data);
  if (scEnvelope.seq !== 0) throw new Error('server_confirm seq is not 0');
  const confirm = await open(scEnvelope.ciphertext, keys.s2c, 0, 2);
  if (confirm.kind !== 'server_confirm' || confirm.protocol !== 2) throw new Error('invalid server_confirm');
  const expected = await hmac(keys.serverConfirm, transcript);
  if (b64(expected) !== confirm.proof) throw new Error('server_confirm proof mismatch');
  const c2sProof = await hmac(keys.clientConfirm, transcript);
  ws.send(JSON.stringify(await seal({ kind: 'client_confirm', proof: b64(c2sProof) }, keys.c2s, 0, 1)));
  const manifest = { kind: 'manifest', provider_id: providerId, version: '2.0.0', browser, capabilities, subscriptions: ['tabs.updated'], limits };
  ws.send(JSON.stringify(await seal(manifest, keys.c2s, 1, 1)));
  const ackFrame = await channel.next(10000);
  if (ackFrame.kind !== 'message') throw new Error('provider closed before manifest_ack');
  const ackEnvelope = JSON.parse(ackFrame.data);
  if (ackEnvelope.seq !== 1) throw new Error('manifest_ack seq is not 1');
  const ack = await open(ackEnvelope.ciphertext, keys.s2c, 1, 2);
  if (ack.kind !== 'manifest_ack' || ack.accepted !== true || ack.provider_id !== providerId) throw new Error('invalid manifest_ack');
  const state = { channel, keys, providerId, manifest, sessionId: confirm.session_id, nextS2c: 2, nextC2s: 2, actions: [], invokes: [], errors: [], stopped: false };
  state.pump = (async () => {
    while (!state.stopped) {
      let event;
      try { event = await channel.next(60000); }
      catch { continue; }
      if (event.kind === 'close') { state.closed = { code: event.code, reason: event.reason }; return; }
      let envelope;
      try { envelope = JSON.parse(event.data); } catch { state.errors.push('non-JSON provider frame'); continue; }
      if (envelope.seq !== state.nextS2c) { state.errors.push('unexpected s2c seq ' + envelope.seq); return; }
      state.nextS2c += 1;
      let message;
      try { message = await open(envelope.ciphertext, keys.s2c, envelope.seq, 2); }
      catch (error) { state.errors.push('decrypt failed: ' + String(error && error.message)); return; }
      if (message.kind !== 'invoke') { state.errors.push('unexpected provider message ' + String(message.kind)); continue; }
      state.invokes.push(message);
      state.actions.push(message.action);
      let response;
      try {
        const value = onInvoke ? await onInvoke(message.action, message.args || {}, message) : defaultInvoke(message.action, message.args || {});
        response = { kind: 'result', id: message.id, ok: true, value };
      } catch (error) {
        response = { kind: 'result', id: message.id, ok: false, error: { code: error.code || 'INTERNAL', message: error.message || 'fake provider error', retryable: false } };
      }
      try {
        ws.send(JSON.stringify(await seal(response, keys.c2s, state.nextC2s, 1)));
        state.nextC2s += 1;
      } catch (error) {
        state.errors.push('result send failed: ' + String(error && error.message));
        return;
      }
    }
  })();
  return state;
}

export function defaultInvoke(action, args) {
  switch (action) {
    case 'tabs.list':
      return { tabs: [
        { tab_id: 2, window_id: 10, title: 'Second', url: 'http://127.0.0.1:8835/second', active: false },
        { tab_id: 1, window_id: 10, title: 'Home', url: 'http://127.0.0.1:8835/page', active: true },
      ], count: 2 };
    case 'tabs.open':
      return { tab_id: 7, window_id: 10, url: args.url, active: args.active !== false };
    case 'tabs.close':
      return { tab_id: args.tab_id, closed: true };
    case 'page.navigate':
      return { tab_id: args.tab_id || '1', url: args.url, title: args.url.endsWith('/second') ? 'Second' : 'P2 Fixture', status: 'loaded' };
    case 'page.read':
      return { tab_id: args.tab_id || '1', url: 'http://127.0.0.1:8835/page', mode: args.mode || 'text', content: 'Hello P2 fixture', truncated: false };
    case 'page.snapshot':
      return { tab_id: args.tab_id || '1', url: 'http://127.0.0.1:8835/page', nodes: [
        { ref: 'private-node-1', role: 'heading', name: 'P2 Fixture', bbox: { x: 8, y: 8, width: 220, height: 28 } },
        { ref: 'private-node-2', role: 'button', name: 'Click me', bbox: { x: 8, y: 60, width: 90, height: 30 } },
        { ref: 'private-node-3', role: 'textbox', name: 'Name', value: 'Alice', bbox: { x: 8, y: 110, width: 160, height: 30 } },
      ], truncated: false };
    case 'input.click':
      return { tab_id: args.tab_id || '1', ref: args.ref, clicked: true };
    case 'input.type':
      return { tab_id: args.tab_id || '1', ref: args.ref, typed_chars: String(args.text || '').length, submitted: !!args.submit };
    case 'capture.viewport':
      return { tab_id: args.tab_id || '1', url: 'http://127.0.0.1:8835/page', format: args.format || 'png', mime_type: args.format === 'jpeg' ? 'image/jpeg' : 'image/png', width: 1, height: 1, data: 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAFgwJ/lK3w5QAAAABJRU5ErkJggg==' };
    default:
      throw Object.assign(new Error('unknown action ' + action), { code: 'BAD_ARGS' });
  }
}
