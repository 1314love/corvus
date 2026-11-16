/**
 * Corvus v2 encrypted provider socket (P1).
 *
 * Library only. The legacy offscreen plaintext socket is untouched and this
 * class is not imported by any runtime entry point yet. P2 will wire it to the
 * generic proxy.
 *
 * Responsibilities:
 * - perform the plaintext server_hello/client_hello exchange;
 * - verify the encrypted server_confirm, send client_confirm and manifest;
 * - enforce strictly increasing AES-GCM frame sequences in both directions;
 * - reject replayed, gapped, tampered or oversized frames with the frozen
 *   WebSocket close codes;
 * - serve system.info.info and tabs.read.list invokes from the gateway;
 * - expose matching request helpers for tests and future callers.
 */
import {
  MAX_HANDSHAKE_FRAME_BYTES,
  MAX_INNER_PLAINTEXT_BYTES,
  MAX_OUTER_FRAME_BYTES,
  MAX_SEQUENCE,
  PROVIDER_CLOSE_CODES,
  PROVIDER_PROTOCOL_VERSION,
  PROVIDER_WS_PATH,
  ProviderProtocolError,
  decodeBase64,
  decodeBase64Url,
  encodeBase64,
  encodeBase64Url,
  parseFrameText,
  parseProviderMessageText,
  parseProviderSystemInfo,
  parseProviderTabs,
  parseServerHelloText,
  serializeMessage,
  utf8ByteLength,
  type ClientConfirm,
  type ClientHello,
  type InvokeMessage,
  type ManifestAck,
  type PingMessage,
  type ProviderManifest,
  type ProviderMessage,
  type ProviderSystemInfo,
  type ProviderTab,
  type ServerConfirm,
  type ServerHello,
  type SubscriptionAck,
} from '../shared/provider/protocol';
import {
  computeSharedSecret,
  confirmationProof,
  createTranscript,
  decryptFrame,
  deriveSessionKeys,
  encryptFrame,
  exportPublicKeyRaw,
  generateP256KeyPair,
  importAesGcmKey,
  pairingProof,
  randomBytes,
  randomUuid,
  verifyConfirmationProof,
  type ProviderSessionKeys,
} from '../shared/provider/crypto';

export const PROVIDER_HANDSHAKE_TIMEOUT_MS = 5000;
export const PROVIDER_REQUEST_TIMEOUT_MS = 30000;

export interface ProviderWebSocketLike {
  readyState: number;
  send(data: string): void;
  close(code?: number, reason?: string): void;
  addEventListener(type: string, listener: (event: any) => void): void;
  removeEventListener?(type: string, listener: (event: any) => void): void;
}

export type ProviderWebSocketFactory = (url: string) => ProviderWebSocketLike;

export interface ProviderBrowserInfo {
  family: string;
  version: string;
  profile: string;
}

export interface ProviderActionHandlers {
  systemInfo: () => ProviderSystemInfo | Promise<ProviderSystemInfo>;
  tabsList: () => ProviderTab[] | Promise<ProviderTab[]>;
}

export interface ProviderEvent {
  topic: string;
  seq: number;
  payload: Record<string, unknown>;
}

export interface ProviderConnectionInfo {
  server_id: string;
  session_id: string;
  provider_id: string;
  browser: ProviderBrowserInfo;
}

export interface ProviderSocketOptions {
  url: string;
  token: string | Uint8Array;
  clientId?: Uint8Array;
  clientKeyPair?: CryptoKeyPair;
  handshakeTimeoutMs?: number;
  browser?: ProviderBrowserInfo;
  extensionVersion?: string;
  actions?: Partial<ProviderActionHandlers>;
  socketFactory?: ProviderWebSocketFactory;
  onEvent?: (event: ProviderEvent) => void;
  onClose?: (error: ProviderSocketError | null) => void;
}

export type ProviderSocketState = 'idle' | 'connecting' | 'ready' | 'closed';

export class ProviderSocketError extends Error {
  readonly code: string;
  readonly closeCode: number | null;

  constructor(code: string, message: string, closeCode: number | null = null) {
    super(message);
    this.name = 'ProviderSocketError';
    this.code = code;
    this.closeCode = closeCode;
  }
}

type Phase = 'idle' | 'await_hello' | 'await_confirm' | 'await_manifest_ack' | 'ready' | 'closed';

interface PendingInvoke {
  id: string;
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

function defaultSocketFactory(url: string): ProviderWebSocketLike {
  return new WebSocket(url) as unknown as ProviderWebSocketLike;
}

function normalizeProviderUrl(raw: string): string {
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    throw new ProviderSocketError('STATE', 'provider URL is invalid');
  }
  if (url.protocol !== 'ws:' && url.protocol !== 'wss:') {
    throw new ProviderSocketError('STATE', 'provider URL must use ws or wss');
  }
  if (url.pathname !== PROVIDER_WS_PATH) {
    throw new ProviderSocketError('STATE', 'provider URL must target ' + PROVIDER_WS_PATH);
  }
  if (url.search || url.hash || url.username || url.password) {
    throw new ProviderSocketError('STATE', 'provider URL must not contain credentials, query or fragment');
  }
  return url.toString();
}

function coerceToken(token: string | Uint8Array): Uint8Array {
  if (typeof token === 'string') {
    try {
      const bytes = decodeBase64Url(token);
      if (bytes.length !== 32) throw new Error('bad length');
      return bytes;
    } catch {
      throw new ProviderSocketError('AUTHENTICATION', 'pairing token is invalid', PROVIDER_CLOSE_CODES.AUTHENTICATION);
    }
  }
  if (!(token instanceof Uint8Array) || token.length !== 32) {
    throw new ProviderSocketError('AUTHENTICATION', 'pairing token is invalid', PROVIDER_CLOSE_CODES.AUTHENTICATION);
  }
  return new Uint8Array(token);
}

function codeForClose(closeCode: number): string {
  switch (closeCode) {
    case PROVIDER_CLOSE_CODES.AUTHENTICATION:
      return 'AUTHENTICATION';
    case PROVIDER_CLOSE_CODES.REPLAY:
      return 'REPLAY';
    case PROVIDER_CLOSE_CODES.VERSION:
      return 'VERSION';
    case PROVIDER_CLOSE_CODES.MALFORMED:
      return 'MALFORMED';
    case PROVIDER_CLOSE_CODES.HANDSHAKE_TIMEOUT:
      return 'HANDSHAKE_TIMEOUT';
    case PROVIDER_CLOSE_CODES.SIZE:
      return 'SIZE';
    default:
      return 'CLOSED';
  }
}

export class ProviderSocket {
  private readonly options: ProviderSocketOptions;
  private readonly browser: ProviderBrowserInfo;
  private readonly extensionVersion: string;
  private phase: Phase = 'idle';
  private socket: ProviderWebSocketLike | null = null;
  private tokenBytes: Uint8Array | null = null;
  private clientIdBytes: Uint8Array | null = null;
  private clientKeyPair: CryptoKeyPair | null = null;
  private clientPublicKeyRaw: Uint8Array | null = null;
  private serverHello: ServerHello | null = null;
  private transcript: Uint8Array | null = null;
  private sessionKeys: ProviderSessionKeys | null = null;
  private aesC2S: CryptoKey | null = null;
  private aesS2C: CryptoKey | null = null;
  private providerId = '';
  private serverId = '';
  private sessionId = '';
  private c2sSeq = 0;
  private s2cNext = 0;
  private handshakeTimer: ReturnType<typeof setTimeout> | null = null;
  private connectPromise: Promise<ProviderConnectionInfo> | null = null;
  private connectResolve: ((value: ProviderConnectionInfo) => void) | null = null;
  private connectReject: ((error: Error) => void) | null = null;
  private pendingInvoke: PendingInvoke | null = null;
  private inboundActive: string | null = null;
  private subscribedTopics = new Set<string>();
  private sendChain: Promise<void> = Promise.resolve();
  private inbox: Promise<void> = Promise.resolve();
  private lastErrorValue: ProviderSocketError | null = null;
  private closedNotified = false;

  constructor(options: ProviderSocketOptions) {
    this.options = options;
    this.browser = options.browser ?? { family: 'unknown', version: 'unknown', profile: 'default' };
    this.extensionVersion = options.extensionVersion ?? '2.0.0';
    this.providerId = 'provider-' + randomUuid();
  }

  get state(): ProviderSocketState {
    if (this.phase === 'idle') return 'idle';
    if (this.phase === 'ready') return 'ready';
    if (this.phase === 'closed') return 'closed';
    return 'connecting';
  }

  get lastError(): ProviderSocketError | null {
    return this.lastErrorValue;
  }

  get connection(): ProviderConnectionInfo | null {
    if (this.phase !== 'ready') return null;
    return {
      server_id: this.serverId,
      session_id: this.sessionId,
      provider_id: this.providerId,
      browser: { ...this.browser },
    };
  }

  connect(): Promise<ProviderConnectionInfo> {
    if (this.connectPromise) return this.connectPromise;
    this.connectPromise = new Promise<ProviderConnectionInfo>((resolve, reject) => {
      this.connectResolve = resolve;
      this.connectReject = reject;
      void this.start().catch((error: unknown) => {
        this.fail(this.toProviderError(error));
      });
    });
    return this.connectPromise;
  }

  async requestSystemInfo(deadlineMs: number = PROVIDER_REQUEST_TIMEOUT_MS): Promise<ProviderSystemInfo> {
    return parseProviderSystemInfo(await this.invoke('system.info.info', {}, deadlineMs));
  }

  async requestTabsList(deadlineMs: number = PROVIDER_REQUEST_TIMEOUT_MS): Promise<ProviderTab[]> {
    return parseProviderTabs(await this.invoke('tabs.read.list', {}, deadlineMs));
  }

  invoke(
    action: string,
    args: Record<string, unknown> = {},
    deadlineMs: number = PROVIDER_REQUEST_TIMEOUT_MS,
  ): Promise<unknown> {
    if (this.phase !== 'ready') {
      return Promise.reject(new ProviderSocketError('STATE', 'provider socket is not ready'));
    }
    if (this.pendingInvoke) {
      return Promise.reject(new ProviderSocketError('STATE', 'another provider invoke is in flight'));
    }
    if (!Number.isInteger(deadlineMs) || deadlineMs < 1 || deadlineMs > 3600000) {
      return Promise.reject(new ProviderSocketError('BAD_ARGS', 'invalid provider deadline'));
    }
    const message: InvokeMessage = {
      kind: 'invoke',
      id: randomUuid(),
      action,
      args,
      deadline_ms: deadlineMs,
    };
    try {
      serializeMessage(message);
    } catch {
      return Promise.reject(new ProviderSocketError('BAD_ARGS', 'invalid provider invoke'));
    }
    return new Promise<unknown>((resolve, reject) => {
      const timer = setTimeout(() => {
        if (this.pendingInvoke && this.pendingInvoke.id === message.id) {
          this.pendingInvoke = null;
          reject(new ProviderSocketError('TIMEOUT', 'provider invoke timed out'));
        }
      }, deadlineMs);
      this.pendingInvoke = { id: message.id, resolve, reject, timer };
      void this.sendEncrypted(message).catch((error: unknown) => {
        if (this.pendingInvoke && this.pendingInvoke.id === message.id) {
          const pending = this.pendingInvoke;
          this.pendingInvoke = null;
          clearTimeout(pending.timer);
          reject(this.toProviderError(error));
        }
      });
    });
  }

  close(code: number = 1000, reason: string = 'provider closed'): void {
    if (this.phase === 'closed') return;
    this.finalize(new ProviderSocketError('CLOSED', 'provider connection closed', code));
    try {
      this.socket?.close(code, reason);
    } catch {
      // The local state is already terminal.
    }
  }

  private async start(): Promise<void> {
    this.tokenBytes = coerceToken(this.options.token);
    const providedClientId = this.options.clientId ? new Uint8Array(this.options.clientId) : null;
    if (providedClientId && (providedClientId.length < 16 || providedClientId.length > 64)) {
      throw new ProviderSocketError(
        'AUTHENTICATION',
        'pairing client id is invalid',
        PROVIDER_CLOSE_CODES.AUTHENTICATION,
      );
    }
    this.clientIdBytes = providedClientId ?? randomBytes(16);
    this.clientKeyPair = this.options.clientKeyPair ?? (await generateP256KeyPair());
    this.clientPublicKeyRaw = await exportPublicKeyRaw(this.clientKeyPair.publicKey);
    const factory = this.options.socketFactory ?? defaultSocketFactory;
    this.socket = factory(normalizeProviderUrl(this.options.url));
    this.phase = 'await_hello';
    this.attachSocket();
    this.armHandshakeTimer();
    if (this.socket.readyState === 1) queueMicrotask(() => this.handleOpen());
  }

  private attachSocket(): void {
    if (!this.socket) return;
    this.socket.addEventListener('open', () => this.handleOpen());
    this.socket.addEventListener('message', (event: any) => this.enqueueIncoming(event?.data));
    this.socket.addEventListener('close', (event: any) => this.handleSocketClose(event));
    this.socket.addEventListener('error', () => this.handleSocketError());
  }

  private handleOpen(): void {
    if (this.phase === 'await_hello') this.armHandshakeTimer();
  }

  private enqueueIncoming(data: unknown): void {
    this.inbox = this.inbox
      .then(() => this.processMessage(data))
      .catch((error: unknown) => {
        this.fail(this.toProviderError(error));
      });
  }

  private async processMessage(data: unknown): Promise<void> {
    if (this.phase === 'closed') return;
    if (typeof data !== 'string') {
      throw new ProviderSocketError('MALFORMED', 'provider message is not text', PROVIDER_CLOSE_CODES.MALFORMED);
    }
    if (this.phase === 'await_hello') {
      await this.handleServerHello(data);
      return;
    }
    if (this.phase === 'await_confirm') {
      await this.handleServerConfirm((await this.decryptNext(data)) as ServerConfirm);
      return;
    }
    if (this.phase === 'await_manifest_ack') {
      await this.handleManifestAck((await this.decryptNext(data)) as ManifestAck);
      return;
    }
    if (this.phase === 'ready') {
      await this.dispatchMessage(await this.decryptNext(data));
      return;
    }
    throw new ProviderSocketError('STATE', 'provider socket is not ready');
  }

  private async handleServerHello(text: string): Promise<void> {
    const hello = parseServerHelloText(text);
    if (hello.protocol !== PROVIDER_PROTOCOL_VERSION) {
      throw new ProviderSocketError('VERSION', 'provider protocol version mismatch', PROVIDER_CLOSE_CODES.VERSION);
    }
    if (!hello.pairing_required) {
      throw new ProviderSocketError(
        'AUTHENTICATION',
        'provider pairing is not enabled',
        PROVIDER_CLOSE_CODES.AUTHENTICATION,
      );
    }
    const token = this.tokenBytes;
    const clientId = this.clientIdBytes;
    const clientPublicKey = this.clientPublicKeyRaw;
    if (!token || !clientId || !clientPublicKey) {
      throw new ProviderSocketError('STATE', 'provider client is not initialised');
    }

    const serverNonce = decodeBase64(hello.server_nonce);
    const serverEphemeral = decodeBase64(hello.server_ephemeral);
    const salt = decodeBase64(hello.salt);
    const clientEphemeral = await generateP256KeyPair();
    const clientNonce = randomBytes(32);
    const clientEphemeralRaw = await exportPublicKeyRaw(clientEphemeral.publicKey);
    const sharedSecret = await computeSharedSecret(clientEphemeral.privateKey, serverEphemeral);
    const transcript = await createTranscript({
      serverNonce,
      clientNonce,
      serverEphemeral,
      clientEphemeral: clientEphemeralRaw,
      clientId,
      clientPublicKey,
    });
    const proof = await pairingProof(token, transcript);
    const clientHello: ClientHello = {
      kind: 'client_hello',
      protocol: PROVIDER_PROTOCOL_VERSION,
      client_id: encodeBase64Url(clientId),
      client_public_key: encodeBase64(clientPublicKey),
      client_nonce: encodeBase64(clientNonce),
      client_ephemeral: encodeBase64(clientEphemeralRaw),
      pairing_proof: encodeBase64(proof),
    };
    this.serverHello = hello;
    this.transcript = transcript;
    this.serverId = hello.server_id;
    this.sessionKeys = await deriveSessionKeys({ salt, sharedSecret, tokenBytes: token });
    this.aesC2S = await importAesGcmKey(this.sessionKeys.c2s);
    this.aesS2C = await importAesGcmKey(this.sessionKeys.s2c);
    this.phase = 'await_confirm';
    this.sendPlain(JSON.stringify(clientHello));
  }

  private async handleServerConfirm(message: ServerConfirm): Promise<void> {
    if (message.kind !== 'server_confirm') {
      throw new ProviderSocketError('MALFORMED', 'expected server_confirm', PROVIDER_CLOSE_CODES.MALFORMED);
    }
    if (message.protocol !== PROVIDER_PROTOCOL_VERSION) {
      throw new ProviderSocketError('VERSION', 'provider protocol version mismatch', PROVIDER_CLOSE_CODES.VERSION);
    }
    const keys = this.sessionKeys;
    const transcript = this.transcript;
    if (!keys || !transcript) {
      throw new ProviderSocketError('STATE', 'provider handshake is incomplete');
    }
    const proof = decodeBase64(message.proof);
    if (!(await verifyConfirmationProof(keys.serverConfirm, transcript, proof))) {
      throw new ProviderSocketError(
        'AUTHENTICATION',
        'provider confirmation failed',
        PROVIDER_CLOSE_CODES.AUTHENTICATION,
      );
    }
    this.sessionId = message.session_id;
    const clientProof = await confirmationProof(keys.clientConfirm, transcript);
    const confirmation: ClientConfirm = {
      kind: 'client_confirm',
      protocol: PROVIDER_PROTOCOL_VERSION,
      proof: encodeBase64(clientProof),
    };
    await this.sendEncrypted(confirmation);
    this.phase = 'await_manifest_ack';
    await this.sendEncrypted(this.buildManifest());
  }

  private async handleManifestAck(message: ManifestAck): Promise<void> {
    if (message.kind !== 'manifest_ack') {
      throw new ProviderSocketError('MALFORMED', 'expected manifest_ack', PROVIDER_CLOSE_CODES.MALFORMED);
    }
    if (!message.accepted) {
      throw new ProviderSocketError(
        'AUTHENTICATION',
        'provider manifest was rejected',
        PROVIDER_CLOSE_CODES.AUTHENTICATION,
      );
    }
    if (message.provider_id !== this.providerId) {
      throw new ProviderSocketError(
        'MALFORMED',
        'provider manifest acknowledgement mismatch',
        PROVIDER_CLOSE_CODES.MALFORMED,
      );
    }
    this.phase = 'ready';
    this.clearHandshakeTimer();
    const resolve = this.connectResolve;
    this.connectResolve = null;
    this.connectReject = null;
    resolve?.({
      server_id: this.serverId,
      session_id: this.sessionId,
      provider_id: this.providerId,
      browser: { ...this.browser },
    });
  }

  private async decryptNext(text: string): Promise<ProviderMessage> {
    if (text.length > MAX_OUTER_FRAME_BYTES) {
      throw new ProviderSocketError('SIZE', 'provider frame is too large', PROVIDER_CLOSE_CODES.SIZE);
    }
    const frame = parseFrameText(text);
    if (frame.seq !== this.s2cNext) {
      throw new ProviderSocketError('REPLAY', 'provider frame sequence mismatch', PROVIDER_CLOSE_CODES.REPLAY);
    }
    const key = this.aesS2C;
    if (!key) throw new ProviderSocketError('STATE', 'provider encryption key is unavailable');
    const plaintext = await decryptFrame(key, decodeBase64(frame.ciphertext), 's2c', frame.seq);
    this.s2cNext = frame.seq + 1;
    return parseProviderMessageText(textDecoder.decode(plaintext));
  }

  private async dispatchMessage(message: ProviderMessage): Promise<void> {
    switch (message.kind) {
      case 'ping':
        await this.sendPong(message);
        return;
      case 'pong':
        return;
      case 'event':
        this.emitEvent(message.topic, message.seq, message.payload);
        return;
      case 'result':
        this.handleResult(message);
        return;
      case 'subscribe':
        await this.handleSubscribe(message);
        return;
      case 'subscription_ack':
        return;
      case 'cancel':
        this.handleCancel(message.id);
        return;
      case 'error':
        this.handleRemoteError(message);
        return;
      case 'invoke':
        void this.handleInvoke(message);
        return;
      case 'server_confirm':
      case 'client_confirm':
      case 'manifest':
      case 'manifest_ack':
        throw new ProviderSocketError('MALFORMED', 'unexpected provider message', PROVIDER_CLOSE_CODES.MALFORMED);
      default:
        throw new ProviderSocketError('MALFORMED', 'unknown provider message', PROVIDER_CLOSE_CODES.MALFORMED);
    }
  }

  private async sendPong(message: PingMessage): Promise<void> {
    await this.sendEncrypted({ kind: 'pong', nonce: message.nonce });
  }

  private async handleSubscribe(message: { kind: 'subscribe'; id: string; topics: string[] }): Promise<void> {
    for (const topic of message.topics) this.subscribedTopics.add(topic);
    const ack: SubscriptionAck = { kind: 'subscription_ack', id: message.id, topics: message.topics };
    await this.sendEncrypted(ack);
  }

  private emitEvent(topic: string, seq: number, payload: Record<string, unknown>): void {
    try {
      this.options.onEvent?.({ topic, seq, payload });
    } catch {
      // Consumer callbacks must not break the transport.
    }
  }

  private handleResult(message: Extract<ProviderMessage, { kind: 'result' }>): void {
    const pending = this.pendingInvoke;
    if (!pending) return;
    if (pending.id !== message.id) {
      throw new ProviderSocketError('MALFORMED', 'provider result id mismatch', PROVIDER_CLOSE_CODES.MALFORMED);
    }
    this.pendingInvoke = null;
    clearTimeout(pending.timer);
    if (message.ok) {
      pending.resolve(message.value);
    } else {
      pending.reject(new ProviderSocketError(message.error.code, message.error.message));
    }
  }

  private handleRemoteError(message: Extract<ProviderMessage, { kind: 'error' }>): void {
    const pending = this.pendingInvoke;
    if (!pending || !message.id || pending.id !== message.id) return;
    this.pendingInvoke = null;
    clearTimeout(pending.timer);
    pending.reject(new ProviderSocketError(message.code, message.message));
  }

  private handleCancel(id: string): void {
    const pending = this.pendingInvoke;
    if (!pending || pending.id !== id) return;
    this.pendingInvoke = null;
    clearTimeout(pending.timer);
    pending.reject(new ProviderSocketError('CANCELLED', 'provider invoke cancelled'));
  }

  private async handleInvoke(message: InvokeMessage): Promise<void> {
    if (this.inboundActive !== null) {
      await this.sendResultError(message.id, 'BAD_ARGS', 'provider is busy with another invoke');
      return;
    }
    this.inboundActive = message.id;
    try {
      let value: unknown;
      try {
        value = await this.runAction(message.action, message.args);
      } catch (error: unknown) {
        const failure = this.actionFailure(error);
        await this.sendResultError(message.id, failure.code, failure.message);
        return;
      }
      await this.sendResultOk(message.id, value);
    } catch (error: unknown) {
      this.fail(this.toProviderError(error));
    } finally {
      if (this.inboundActive === message.id) this.inboundActive = null;
    }
  }

  private async runAction(action: string, _args: Record<string, unknown>): Promise<unknown> {
    const separator = action.lastIndexOf('.');
    if (separator <= 0 || separator === action.length - 1) {
      throw new ProviderSocketError('BAD_ARGS', 'invalid provider action');
    }
    const capability = action.slice(0, separator);
    const operation = action.slice(separator + 1);
    if (capability === 'system.info') {
      if (operation !== 'info') throw new ProviderSocketError('BAD_ARGS', 'invalid provider action');
      return this.collectSystemInfo();
    }
    if (capability === 'tabs.read') {
      if (operation !== 'list') throw new ProviderSocketError('BAD_ARGS', 'invalid provider action');
      return this.collectTabs();
    }
    throw new ProviderSocketError('CAPABILITY_UNAVAILABLE', 'provider capability is not available');
  }

  private async collectSystemInfo(): Promise<ProviderSystemInfo> {
    const handler = this.options.actions?.systemInfo;
    const value = handler
      ? await handler()
      : {
          browser_family: this.browser.family,
          browser_version: this.browser.version,
          extension_version: this.extensionVersion,
          capabilities: ['system.info', 'tabs.read'],
        };
    return parseProviderSystemInfo(value);
  }

  private async collectTabs(): Promise<ProviderTab[]> {
    const handler = this.options.actions?.tabsList;
    const value = handler ? await handler() : [];
    if (!Array.isArray(value)) return parseProviderTabs(value);
    return parseProviderTabs(value.slice(0, 200));
  }

  private actionFailure(error: unknown): { code: string; message: string } {
    if (error instanceof ProviderSocketError) {
      if (error.code === 'BAD_ARGS') return { code: 'BAD_ARGS', message: error.message };
      if (error.code === 'CAPABILITY_UNAVAILABLE') {
        return { code: 'CAPABILITY_UNAVAILABLE', message: error.message };
      }
    }
    return { code: 'INTERNAL', message: 'provider action failed' };
  }

  private async sendResultOk(id: string, value: unknown): Promise<void> {
    await this.sendEncrypted({ kind: 'result', id, ok: true, value: value === undefined ? null : value });
  }

  private async sendResultError(id: string, code: string, message: string): Promise<void> {
    await this.sendEncrypted({
      kind: 'result',
      id,
      ok: false,
      error: { code, message, retryable: false },
    });
  }

  private buildManifest(): ProviderManifest {
    return {
      kind: 'manifest',
      provider_id: this.providerId,
      version: '2.0.0',
      browser: { ...this.browser },
      capabilities: [
        { id: 'system.info', version: 1, actions: ['info'] },
        { id: 'tabs.read', version: 1, actions: ['list'] },
      ],
      subscriptions: ['tabs.updated'],
      limits: { max_invoke_bytes: 1048576, max_result_bytes: 8388608 },
    };
  }

  private sendPlain(text: string): void {
    if (utf8ByteLength(text) > MAX_HANDSHAKE_FRAME_BYTES) {
      throw new ProviderSocketError('SIZE', 'provider handshake is too large', PROVIDER_CLOSE_CODES.SIZE);
    }
    if (!this.socket) throw new ProviderSocketError('STATE', 'provider socket is unavailable');
    try {
      this.socket.send(text);
    } catch {
      throw new ProviderSocketError('NETWORK', 'provider transport failed');
    }
  }

  private sendEncrypted(message: ProviderMessage): Promise<void> {
    const run = this.sendChain.then(() => this.sendEncryptedNow(message));
    this.sendChain = run.catch(() => undefined);
    return run;
  }

  private async sendEncryptedNow(message: ProviderMessage): Promise<void> {
    if (this.phase === 'closed') {
      throw new ProviderSocketError('CLOSED', 'provider connection is closed');
    }
    if (!this.socket) throw new ProviderSocketError('STATE', 'provider socket is unavailable');
    const plaintext = textEncoder.encode(serializeMessage(message));
    if (plaintext.length > MAX_INNER_PLAINTEXT_BYTES) {
      throw new ProviderSocketError('SIZE', 'provider message is too large', PROVIDER_CLOSE_CODES.SIZE);
    }
    const key = this.aesC2S;
    if (!key) throw new ProviderSocketError('STATE', 'provider encryption key is unavailable');
    if (this.c2sSeq > MAX_SEQUENCE) {
      throw new ProviderSocketError('REPLAY', 'provider frame sequence overflow', PROVIDER_CLOSE_CODES.REPLAY);
    }
    const seq = this.c2sSeq;
    const ciphertext = await encryptFrame(key, plaintext, 'c2s', seq);
    const text = JSON.stringify({ seq, ciphertext: encodeBase64(ciphertext) });
    if (utf8ByteLength(text) > MAX_OUTER_FRAME_BYTES) {
      throw new ProviderSocketError('SIZE', 'provider frame is too large', PROVIDER_CLOSE_CODES.SIZE);
    }
    try {
      this.socket.send(text);
    } catch {
      throw new ProviderSocketError('NETWORK', 'provider transport failed');
    }
    this.c2sSeq = seq + 1;
  }

  private armHandshakeTimer(): void {
    this.clearHandshakeTimer();
    const timeout = this.options.handshakeTimeoutMs ?? PROVIDER_HANDSHAKE_TIMEOUT_MS;
    this.handshakeTimer = setTimeout(() => {
      this.fail(
        new ProviderSocketError(
          'HANDSHAKE_TIMEOUT',
          'provider handshake timed out',
          PROVIDER_CLOSE_CODES.HANDSHAKE_TIMEOUT,
        ),
      );
    }, timeout);
  }

  private clearHandshakeTimer(): void {
    if (this.handshakeTimer !== null) {
      clearTimeout(this.handshakeTimer);
      this.handshakeTimer = null;
    }
  }

  private handleSocketClose(event: any): void {
    if (this.phase === 'closed') return;
    const closeCode = typeof event?.code === 'number' ? event.code : 1006;
    this.finalize(new ProviderSocketError(codeForClose(closeCode), 'provider connection closed', closeCode));
  }

  private handleSocketError(): void {
    this.fail(new ProviderSocketError('NETWORK', 'provider transport failed'));
  }

  private fail(error: ProviderSocketError): void {
    if (this.phase === 'closed') return;
    this.finalize(error);
    try {
      this.socket?.close(error.closeCode ?? 1000, error.code);
    } catch {
      // The local state is already terminal.
    }
  }

  private finalize(error: ProviderSocketError): void {
    if (this.phase === 'closed') return;
    this.phase = 'closed';
    this.clearHandshakeTimer();
    this.lastErrorValue = error;
    const reject = this.connectReject;
    this.connectResolve = null;
    this.connectReject = null;
    if (reject) reject(error);
    this.rejectPendingInvoke(error);
    if (!this.closedNotified) {
      this.closedNotified = true;
      try {
        this.options.onClose?.(error);
      } catch {
        // Consumer callbacks must not break the transport.
      }
    }
  }

  private rejectPendingInvoke(error: ProviderSocketError): void {
    const pending = this.pendingInvoke;
    if (!pending) return;
    this.pendingInvoke = null;
    clearTimeout(pending.timer);
    pending.reject(error);
  }

  private toProviderError(error: unknown): ProviderSocketError {
    if (error instanceof ProviderSocketError) return error;
    if (error instanceof ProviderProtocolError) {
      return new ProviderSocketError(codeForClose(error.closeCode), error.message, error.closeCode);
    }
    return new ProviderSocketError('NETWORK', 'provider transport failed');
  }
}
