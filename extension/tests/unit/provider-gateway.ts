import {
  ProviderProtocolError,
  decodeBase64,
  decodeBase64Url,
  encodeBase64,
  encodeBase64Url,
  parseClientHelloText,
  parseFrameText,
  parseManifest,
  parseProviderMessageText,
  serializeMessage,
  type ProviderManifest,
  type ProviderMessage,
  type ProviderSystemInfo,
  type ProviderTab,
} from '../../src/shared/provider/protocol';
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
  randomBytes,
  randomUuid,
  verifyConfirmationProof,
  verifyPairingProof,
  type ProviderSessionKeys,
} from '../../src/shared/provider/crypto';
import type { ProviderWebSocketLike } from '../../src/offscreen/provider-socket';

const encoder = new TextEncoder();
const decoder = new TextDecoder();

type Listener = (event: any) => void;

export class TestEndpoint implements ProviderWebSocketLike {
  readyState = 0;
  peer: TestEndpoint | null = null;
  private readonly listeners = new Map<string, Set<Listener>>();

  send(data: string): void {
    if (this.readyState !== 1) throw new Error('test socket is not open');
    queueMicrotask(() => {
      this.peer?.emit('message', { data });
    });
  }

  close(code: number = 1000, reason: string = ''): void {
    if (this.readyState === 3) return;
    this.readyState = 3;
    queueMicrotask(() => {
      this.peer?.emit('close', { code, reason });
      this.emit('close', { code, reason });
    });
  }

  addEventListener(type: string, listener: Listener): void {
    const current = this.listeners.get(type) ?? new Set<Listener>();
    current.add(listener);
    this.listeners.set(type, current);
  }

  removeEventListener(type: string, listener: Listener): void {
    this.listeners.get(type)?.delete(listener);
  }

  open(): void {
    if (this.readyState !== 0) return;
    this.readyState = 1;
    this.emit('open', {});
  }

  emit(type: string, event: any): void {
    for (const listener of this.listeners.get(type) ?? []) listener(event);
  }
}

export class TestSocketPair {
  readonly client = new TestEndpoint();
  readonly server = new TestEndpoint();

  constructor() {
    this.client.peer = this.server;
    this.server.peer = this.client;
  }

  open(): void {
    this.server.open();
    this.client.open();
  }

  dispose(): void {
    if (this.client.readyState !== 3) this.client.readyState = 3;
    if (this.server.readyState !== 3) this.server.readyState = 3;
  }
}

export interface TestGatewayOptions {
  token?: Uint8Array;
  protocol?: number;
  wrongConfirm?: boolean;
  autoHello?: boolean;
  systemInfo?: ProviderSystemInfo;
  tabs?: ProviderTab[];
}

interface PendingCall {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

export class TestGateway {
  readonly token: Uint8Array;
  readonly pair = new TestSocketPair();
  readonly protocol: number;
  readonly wrongConfirm: boolean;
  readonly autoHello: boolean;
  readonly systemInfo: ProviderSystemInfo;
  readonly tabs: ProviderTab[];
  readonly sessionId: string;
  serverId = '';
  manifest: ProviderManifest | null = null;
  closeCode: number | null = null;

  private serverKeyPair: CryptoKeyPair | null = null;
  private serverEphemeralRaw: Uint8Array | null = null;
  private readonly serverNonce = randomBytes(32);
  private readonly salt = randomBytes(32);
  private readonly serverIdBytes = randomBytes(16);
  private phase: 'hello' | 'confirm' | 'manifest' | 'ready' | 'closed' = 'hello';
  private transcript: Uint8Array | null = null;
  private keys: ProviderSessionKeys | null = null;
  private aesC2S: CryptoKey | null = null;
  private aesS2C: CryptoKey | null = null;
  private c2sNext = 0;
  private s2cNext = 0;
  private helloSent = false;
  private inbox: Promise<void> = Promise.resolve();
  private sendChain: Promise<void> = Promise.resolve();
  private readonly pending = new Map<string, PendingCall>();
  private lastFrameText: string | null = null;

  private constructor(options: TestGatewayOptions) {
    this.token = options.token
      ? new Uint8Array(options.token)
      : Uint8Array.from({ length: 32 }, (_, index) => index + 1);
    this.protocol = options.protocol ?? 2;
    this.wrongConfirm = options.wrongConfirm ?? false;
    this.autoHello = options.autoHello ?? true;
    this.systemInfo = options.systemInfo ?? {
      browser_family: 'unknown',
      browser_version: 'unknown',
      extension_version: '2.0.0',
      capabilities: ['system.info', 'tabs'],
    };
    this.tabs = options.tabs ?? [
      {
        tab_id: '7',
        window_id: 1,
        title: 'Example',
        url: 'https://example.com/',
        active: true,
      },
    ];
    this.sessionId = 's-' + randomUuid();
    this.pair.server.addEventListener('open', () => {
      if (this.autoHello) void this.sendHello();
    });
    this.pair.server.addEventListener('message', (event: any) => this.enqueue(event?.data));
    this.pair.server.addEventListener('close', (event: any) => {
      this.closeCode = typeof event?.code === 'number' ? event.code : 1006;
      this.phase = 'closed';
    });
  }

  static async create(options: TestGatewayOptions = {}): Promise<TestGateway> {
    const gateway = new TestGateway(options);
    gateway.serverKeyPair = await generateP256KeyPair();
    gateway.serverEphemeralRaw = await exportPublicKeyRaw(gateway.serverKeyPair.publicKey);
    gateway.serverId = encodeBase64Url(gateway.serverIdBytes);
    return gateway;
  }

  async sendEvent(payload: Record<string, unknown> = { ok: true }): Promise<void> {
    await this.sendEncrypted({ kind: 'event', topic: 'tabs.updated', seq: 1, payload });
  }

  async sendTamperedEvent(): Promise<void> {
    const frame = await this.encryptAt(
      { kind: 'event', topic: 'tabs.updated', seq: 1, payload: { tampered: true } },
      this.s2cNext,
    );
    const bytes = decodeBase64(frame.ciphertext);
    bytes[bytes.length - 1] ^= 1;
    this.pair.server.send(JSON.stringify({ seq: frame.seq, ciphertext: encodeBase64(bytes) }));
  }

  async sendGapEvent(): Promise<void> {
    const frame = await this.encryptAt(
      { kind: 'event', topic: 'tabs.updated', seq: 1, payload: { gap: true } },
      this.s2cNext + 1,
    );
    this.pair.server.send(JSON.stringify(frame));
  }

  async replayLastFrame(): Promise<void> {
    if (this.lastFrameText === null) throw new Error('no frame to replay');
    this.pair.server.send(this.lastFrameText);
  }

  invoke(action: string, args: Record<string, unknown> = {}, deadlineMs: number = 2000): Promise<unknown> {
    if (this.phase !== 'ready') return Promise.reject(new Error('gateway is not ready'));
    const id = randomUuid();
    return new Promise<unknown>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error('gateway invoke timed out'));
      }, deadlineMs);
      this.pending.set(id, { resolve, reject, timer });
      void this.sendEncrypted({ kind: 'invoke', id, action, args, deadline_ms: deadlineMs }).catch((error) => {
        this.pending.delete(id);
        clearTimeout(timer);
        reject(error instanceof Error ? error : new Error('gateway send failed'));
      });
    });
  }

  dispose(): void {
    this.phase = 'closed';
    this.pair.dispose();
  }

  private async sendHello(): Promise<void> {
    if (this.helloSent || !this.serverEphemeralRaw) return;
    this.helloSent = true;
    this.pair.server.send(
      JSON.stringify({
        kind: 'server_hello',
        protocol: this.protocol,
        server_id: this.serverId,
        server_nonce: encodeBase64(this.serverNonce),
        server_ephemeral: encodeBase64(this.serverEphemeralRaw),
        salt: encodeBase64(this.salt),
        pairing_required: true,
      }),
    );
  }

  private enqueue(data: unknown): void {
    this.inbox = this.inbox.then(() => this.processMessage(data)).catch((error: unknown) => this.fail(error));
  }

  private async processMessage(data: unknown): Promise<void> {
    if (this.phase === 'closed') return;
    if (typeof data !== 'string') throw new ProviderProtocolError('provider frame is not text');
    if (this.phase === 'hello') {
      await this.handleClientHello(data);
      return;
    }
    await this.handleEncrypted(data);
  }

  private async handleClientHello(text: string): Promise<void> {
    const hello = parseClientHelloText(text);
    if (hello.protocol !== 2) {
      this.close(4403);
      return;
    }
    const serverEphemeral = this.serverEphemeralRaw;
    const serverPrivateKey = this.serverKeyPair?.privateKey;
    if (!serverEphemeral || !serverPrivateKey) throw new ProviderProtocolError('gateway is not initialised');
    const clientEphemeral = decodeBase64(hello.client_ephemeral);
    const clientId = decodeBase64Url(hello.client_id);
    const transcript = await createTranscript({
      serverNonce: this.serverNonce,
      clientNonce: decodeBase64(hello.client_nonce),
      serverEphemeral,
      clientEphemeral,
      clientId,
      clientPublicKey: decodeBase64(hello.client_public_key),
    });
    const proof = decodeBase64(hello.pairing_proof);
    if (!(await verifyPairingProof(this.token, transcript, proof))) {
      this.close(4401);
      return;
    }
    const shared = await computeSharedSecret(serverPrivateKey, clientEphemeral);
    const keys = await deriveSessionKeys({ salt: this.salt, sharedSecret: shared, tokenBytes: this.token });
    this.transcript = transcript;
    this.keys = keys;
    this.aesC2S = await importAesGcmKey(keys.c2s);
    this.aesS2C = await importAesGcmKey(keys.s2c);
    this.phase = 'confirm';
    const confirmation = this.wrongConfirm ? randomBytes(32) : await confirmationProof(keys.serverConfirm, transcript);
    await this.sendEncrypted({
      kind: 'server_confirm',
      protocol: 2,
      session_id: this.sessionId,
      proof: encodeBase64(confirmation),
    });
  }

  private async handleEncrypted(text: string): Promise<void> {
    const frame = parseFrameText(text);
    if (frame.seq !== this.c2sNext) {
      this.close(4402);
      return;
    }
    const key = this.aesC2S;
    if (!key) throw new ProviderProtocolError('gateway encryption key is unavailable');
    let plaintext: Uint8Array;
    try {
      plaintext = await decryptFrame(key, decodeBase64(frame.ciphertext), 'c2s', frame.seq);
    } catch {
      this.close(4404);
      return;
    }
    this.c2sNext = frame.seq + 1;
    const message = parseProviderMessageText(decoder.decode(plaintext));
    if (this.phase === 'confirm') {
      if (message.kind !== 'client_confirm' || !this.keys || !this.transcript) {
        this.close(4404);
        return;
      }
      const valid = await verifyConfirmationProof(
        this.keys.clientConfirm,
        this.transcript,
        decodeBase64(message.proof),
      );
      if (!valid) {
        this.close(4401);
        return;
      }
      this.phase = 'manifest';
      return;
    }
    if (this.phase === 'manifest') {
      if (message.kind !== 'manifest') {
        this.close(4404);
        return;
      }
      this.manifest = parseManifest(message);
      this.phase = 'ready';
      await this.sendEncrypted({
        kind: 'manifest_ack',
        provider_id: this.manifest.provider_id,
        accepted: true,
      });
      return;
    }
    await this.dispatch(message);
  }

  private async dispatch(message: ProviderMessage): Promise<void> {
    if (message.kind === 'invoke') {
      await this.serveInvoke(message);
      return;
    }
    if (message.kind === 'result') {
      const pending = this.pending.get(message.id);
      if (!pending) return;
      this.pending.delete(message.id);
      clearTimeout(pending.timer);
      if (message.ok) pending.resolve(message.value);
      else pending.reject(new Error(message.error.message));
      return;
    }
    if (message.kind === 'ping') {
      await this.sendEncrypted({ kind: 'pong', nonce: message.nonce });
      return;
    }
    if (message.kind === 'subscribe') {
      await this.sendEncrypted({ kind: 'subscription_ack', id: message.id, topics: message.topics });
      return;
    }
    if (message.kind === 'error') {
      const pending = message.id ? this.pending.get(message.id) : undefined;
      if (!pending) return;
      this.pending.delete(message.id as string);
      clearTimeout(pending.timer);
      pending.reject(new Error(message.message));
    }
  }

  private async serveInvoke(message: Extract<ProviderMessage, { kind: 'invoke' }>): Promise<void> {
    try {
      let value: unknown;
      if (message.action === 'system.info.info') value = this.systemInfo;
      else if (message.action === 'tabs.list') value = this.tabs;
      else {
        await this.sendEncrypted({
          kind: 'result',
          id: message.id,
          ok: false,
          error: { code: 'CAPABILITY_UNAVAILABLE', message: 'unsupported test action', retryable: false },
        });
        return;
      }
      await this.sendEncrypted({ kind: 'result', id: message.id, ok: true, value });
    } catch {
      await this.sendEncrypted({
        kind: 'result',
        id: message.id,
        ok: false,
        error: { code: 'INTERNAL', message: 'test action failed', retryable: false },
      });
    }
  }

  private sendEncrypted(message: ProviderMessage): Promise<void> {
    const run = this.sendChain.then(async () => {
      const frame = await this.encryptAt(message, this.s2cNext);
      this.s2cNext += 1;
      const text = JSON.stringify(frame);
      this.lastFrameText = text;
      this.pair.server.send(text);
    });
    this.sendChain = run.catch(() => undefined);
    return run;
  }

  private async encryptAt(message: ProviderMessage, seq: number): Promise<{ seq: number; ciphertext: string }> {
    const key = this.aesS2C;
    if (!key) throw new ProviderProtocolError('gateway encryption key is unavailable');
    const plaintext = encoder.encode(serializeMessage(message));
    const ciphertext = await encryptFrame(key, plaintext, 's2c', seq);
    return { seq, ciphertext: encodeBase64(ciphertext) };
  }

  private fail(error: unknown): void {
    const closeCode = error instanceof ProviderProtocolError ? error.closeCode : 4404;
    this.close(closeCode);
  }

  private close(code: number): void {
    if (this.phase === 'closed') return;
    this.phase = 'closed';
    this.closeCode = code;
    this.pair.server.close(code, 'test gateway closed');
  }
}
