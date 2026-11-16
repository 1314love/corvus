/**
 * Corvus v2 provider wire protocol (P1).
 *
 * This module is a library only: it defines the typed messages, the strict
 * runtime validators and the small amount of framing/encoding shared by the
 * WebCrypto client. Nothing here is wired into the v1.7 command map yet.
 *
 * The validators intentionally reject unknown keys and oversize values before
 * any cryptographic work. Error messages are field-oriented and never include
 * the rejected value, so a pairing token cannot leak through a thrown error.
 */
import { z } from 'zod';

export const PROVIDER_PROTOCOL_VERSION = 2 as const;
export const PROVIDER_WS_PATH = '/provider/v2';
export const MAX_HANDSHAKE_FRAME_BYTES = 4096;
export const MAX_INNER_PLAINTEXT_BYTES = 16 * 1024 * 1024;
export const MAX_OUTER_FRAME_BYTES = 24 * 1024 * 1024;
export const MAX_CIPHERTEXT_BYTES = MAX_INNER_PLAINTEXT_BYTES + 16;
export const MAX_SEQUENCE = Number.MAX_SAFE_INTEGER;

export const PROVIDER_CLOSE_CODES = Object.freeze({
  AUTHENTICATION: 4401,
  REPLAY: 4402,
  VERSION: 4403,
  MALFORMED: 4404,
  HANDSHAKE_TIMEOUT: 4408,
  SIZE: 1013,
});

export type ProviderCloseCode = (typeof PROVIDER_CLOSE_CODES)[keyof typeof PROVIDER_CLOSE_CODES];

/** A safe, value-free protocol validation error. */
export class ProviderProtocolError extends Error {
  readonly code = 'PROTOCOL_INVALID' as const;
  readonly closeCode: number;

  constructor(message = 'invalid provider message', closeCode: number = PROVIDER_CLOSE_CODES.MALFORMED) {
    super(message);
    this.name = 'ProviderProtocolError';
    this.closeCode = closeCode;
  }
}

const BASE64_RE = /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/;
const BASE64URL_RE = /^[A-Za-z0-9_-]*={0,2}$/;
const UUID_RE = /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/;

export function isUuid(value: unknown): value is string {
  return typeof value === 'string' && UUID_RE.test(value);
}

export function isBase64(value: unknown): value is string {
  return typeof value === 'string' && BASE64_RE.test(value);
}

export function isBase64Url(value: unknown): value is string {
  return typeof value === 'string' && BASE64URL_RE.test(value) && value.length % 4 !== 1;
}

/** Decoded byte length of a canonical base64 string without allocating it. */
export function base64DecodedLength(value: string): number {
  if (!isBase64(value)) throw new ProviderProtocolError('invalid base64 encoding');
  const body = value.replace(/=+$/, '');
  return Math.floor((body.length * 3) / 4);
}

export function decodeBase64(value: string): Uint8Array {
  if (!isBase64(value)) throw new ProviderProtocolError('invalid base64 encoding');
  let binary: string;
  try {
    binary = atob(value);
  } catch {
    throw new ProviderProtocolError('invalid base64 encoding');
  }
  const out = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) out[index] = binary.charCodeAt(index);
  return out;
}

export function encodeBase64(bytes: Uint8Array): string {
  let binary = '';
  const chunk = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += chunk) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunk));
  }
  return btoa(binary);
}

export function decodeBase64Url(value: string): Uint8Array {
  if (!isBase64Url(value)) throw new ProviderProtocolError('invalid base64url encoding');
  const normalized = value.replace(/-/g, '+').replace(/_/g, '/');
  const padded = normalized + '='.repeat((4 - (normalized.length % 4)) % 4);
  return decodeBase64(padded);
}

export function encodeBase64Url(bytes: Uint8Array): string {
  return encodeBase64(bytes).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

export function utf8ByteLength(value: string): number {
  return new TextEncoder().encode(value).length;
}

const maxString = (length: number) => z.string().max(length);
const nonEmptyString = (length: number) => z.string().min(1).max(length);
const integer = (min: number, max: number) => z.number().int().min(min).max(max);
const uuidSchema = z.string().refine(isUuid, { message: 'must be a UUID' });
const protocolNumber = integer(1, 1000);
const sequenceNumber = integer(1, MAX_SEQUENCE);

function base64Length(length: number): z.ZodType<string> {
  return z.string().refine(
    (value) => {
      try {
        return base64DecodedLength(value) === length;
      } catch {
        return false;
      }
    },
    { message: `must be base64 for exactly ${length} bytes` },
  );
}

function base64UrlRange(min: number, max: number): z.ZodType<string> {
  return z.string().refine(
    (value) => {
      try {
        const length = decodeBase64Url(value).length;
        return length >= min && length <= max;
      } catch {
        return false;
      }
    },
    { message: `must be base64url for ${min}..${max} bytes` },
  );
}

const p256PublicKey = z.string().refine(
  (value) => {
    try {
      const bytes = decodeBase64(value);
      return bytes.length === 65 && bytes[0] === 0x04;
    } catch {
      return false;
    }
  },
  { message: 'must be a 65-byte uncompressed P-256 public key' },
);

const jsonObject = z
  .record(z.string(), z.unknown())
  .refine((value) => Object.keys(value).length <= 1024, { message: 'must have at most 1024 keys' });

const providerIdSchema = z
  .string()
  .regex(/^provider-[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/);
const sessionIdSchema = z
  .string()
  .regex(/^s-[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/);
const capabilityIdSchema = z.string().regex(/^[a-z][a-z0-9_.-]{0,63}$/);
const actionNameSchema = z.string().regex(/^[a-z][a-z0-9_.-]{0,127}$/);
const topicSchema = z.string().regex(/^[a-z][a-z0-9_.-]{0,127}$/);

export const serverHelloSchema = z
  .object({
    kind: z.literal('server_hello'),
    protocol: protocolNumber,
    server_id: base64UrlRange(16, 16),
    server_nonce: base64Length(32),
    server_ephemeral: p256PublicKey,
    salt: base64Length(32),
    pairing_required: z.boolean(),
  })
  .strict();

export const clientHelloSchema = z
  .object({
    kind: z.literal('client_hello'),
    protocol: protocolNumber,
    client_id: base64UrlRange(16, 64),
    client_public_key: p256PublicKey,
    client_nonce: base64Length(32),
    client_ephemeral: p256PublicKey,
    pairing_proof: base64Length(32),
  })
  .strict();

export const serverConfirmSchema = z
  .object({
    kind: z.literal('server_confirm'),
    protocol: protocolNumber,
    session_id: sessionIdSchema,
    proof: base64Length(32),
  })
  .strict();

export const clientConfirmSchema = z
  .object({
    kind: z.literal('client_confirm'),
    protocol: protocolNumber,
    proof: base64Length(32),
  })
  .strict();

export const encryptedFrameSchema = z
  .object({
    seq: integer(0, MAX_SEQUENCE),
    ciphertext: z.string().refine(
      (value) => {
        try {
          const length = base64DecodedLength(value);
          return length >= 16 && length <= MAX_CIPHERTEXT_BYTES;
        } catch {
          return false;
        }
      },
      { message: 'must be base64 ciphertext with a 16-byte GCM tag' },
    ),
  })
  .strict();

const browserSchema = z
  .object({
    family: z
      .string()
      .min(1)
      .max(32)
      .regex(/^[A-Za-z0-9]+$/),
    version: nonEmptyString(64),
    profile: nonEmptyString(64),
  })
  .strict();

const capabilityActionSchema = z.string().regex(/^[a-z0-9_]{1,64}$/);

const capabilitySchema = z
  .object({
    id: capabilityIdSchema,
    version: integer(1, 1000),
    actions: z.array(capabilityActionSchema).min(1).max(32),
  })
  .strict();

export const manifestSchema = z
  .object({
    kind: z.literal('manifest'),
    provider_id: providerIdSchema,
    version: z.literal('2.0.0'),
    browser: browserSchema,
    capabilities: z.array(capabilitySchema).min(1).max(32),
    subscriptions: z.array(z.literal('tabs.updated')).max(8),
    limits: z
      .object({
        max_invoke_bytes: integer(1, 16 * 1024 * 1024),
        max_result_bytes: integer(1, 24 * 1024 * 1024),
      })
      .strict(),
  })
  .strict();

export const manifestAckSchema = z
  .object({
    kind: z.literal('manifest_ack'),
    provider_id: providerIdSchema,
    accepted: z.boolean(),
  })
  .strict();

const errorInfoSchema = z
  .object({
    code: z.string().regex(/^[A-Z][A-Z0-9_]{0,63}$/),
    message: nonEmptyString(2000),
    retryable: z.boolean(),
  })
  .strict();

export const invokeSchema = z
  .object({
    kind: z.literal('invoke'),
    id: uuidSchema,
    action: actionNameSchema,
    args: jsonObject,
    deadline_ms: integer(1, 3600000),
  })
  .strict();

const resultOkSchema = z
  .object({
    kind: z.literal('result'),
    id: uuidSchema,
    ok: z.literal(true),
    value: z.unknown(),
  })
  .strict()
  .refine((value) => Object.prototype.hasOwnProperty.call(value, 'value'), { message: 'value is required' });

const resultErrorSchema = z
  .object({
    kind: z.literal('result'),
    id: uuidSchema,
    ok: z.literal(false),
    error: errorInfoSchema,
  })
  .strict();

export const subscribeSchema = z
  .object({
    kind: z.literal('subscribe'),
    id: uuidSchema,
    topics: z.array(topicSchema).min(1).max(32),
  })
  .strict();

export const subscriptionAckSchema = z
  .object({
    kind: z.literal('subscription_ack'),
    id: uuidSchema,
    topics: z.array(topicSchema).min(1).max(32),
  })
  .strict();

export const eventSchema = z
  .object({
    kind: z.literal('event'),
    topic: topicSchema,
    seq: sequenceNumber,
    payload: jsonObject,
  })
  .strict();

export const cancelSchema = z
  .object({
    kind: z.literal('cancel'),
    id: uuidSchema,
  })
  .strict();

export const pingSchema = z
  .object({
    kind: z.literal('ping'),
    nonce: nonEmptyString(256),
  })
  .strict();

export const pongSchema = z
  .object({
    kind: z.literal('pong'),
    nonce: nonEmptyString(256),
  })
  .strict();

export const providerErrorSchema = z
  .object({
    kind: z.literal('error'),
    id: uuidSchema.optional(),
    code: z.string().regex(/^[A-Z][A-Z0-9_]{0,63}$/),
    message: nonEmptyString(2000),
  })
  .strict();

export const providerMessageSchema = z.union([
  serverConfirmSchema,
  clientConfirmSchema,
  manifestSchema,
  manifestAckSchema,
  invokeSchema,
  resultOkSchema,
  resultErrorSchema,
  subscribeSchema,
  subscriptionAckSchema,
  eventSchema,
  cancelSchema,
  pingSchema,
  pongSchema,
  providerErrorSchema,
]);

export type ServerHello = z.infer<typeof serverHelloSchema>;
export type ClientHello = z.infer<typeof clientHelloSchema>;
export type ServerConfirm = z.infer<typeof serverConfirmSchema>;
export type ClientConfirm = z.infer<typeof clientConfirmSchema>;
export type EncryptedFrame = z.infer<typeof encryptedFrameSchema>;
export type ProviderManifest = z.infer<typeof manifestSchema>;
export type ManifestAck = z.infer<typeof manifestAckSchema>;
export type ProviderErrorInfo = z.infer<typeof errorInfoSchema>;
export type InvokeMessage = z.infer<typeof invokeSchema>;
export type ResultMessage = z.infer<typeof resultOkSchema> | z.infer<typeof resultErrorSchema>;
export type SubscribeMessage = z.infer<typeof subscribeSchema>;
export type SubscriptionAck = z.infer<typeof subscriptionAckSchema>;
export type EventMessage = z.infer<typeof eventSchema>;
export type CancelMessage = z.infer<typeof cancelSchema>;
export type PingMessage = z.infer<typeof pingSchema>;
export type PongMessage = z.infer<typeof pongSchema>;
export type ProviderErrorMessage = z.infer<typeof providerErrorSchema>;
export type ProviderMessage =
  | ServerConfirm
  | ClientConfirm
  | ProviderManifest
  | ManifestAck
  | InvokeMessage
  | ResultMessage
  | SubscribeMessage
  | SubscriptionAck
  | EventMessage
  | CancelMessage
  | PingMessage
  | PongMessage
  | ProviderErrorMessage;

function parseValue<T>(schema: z.ZodType<T>, value: unknown, field: string): T {
  const result = schema.safeParse(value);
  if (result.success) return result.data;
  const first = result.error.issues[0];
  const path = first && first.path.length > 0 ? ` at ${first.path.join('.')}` : '';
  throw new ProviderProtocolError(`invalid ${field}${path}`);
}

export function parseServerHello(value: unknown): ServerHello {
  return parseValue(serverHelloSchema, value, 'server_hello');
}

export function parseClientHello(value: unknown): ClientHello {
  return parseValue(clientHelloSchema, value, 'client_hello');
}

export function parseServerConfirm(value: unknown): ServerConfirm {
  return parseValue(serverConfirmSchema, value, 'server_confirm');
}

export function parseClientConfirm(value: unknown): ClientConfirm {
  return parseValue(clientConfirmSchema, value, 'client_confirm');
}

export function parseFrame(value: unknown): EncryptedFrame {
  return parseValue(encryptedFrameSchema, value, 'frame');
}

export function parseManifest(value: unknown): ProviderManifest {
  return parseValue(manifestSchema, value, 'manifest');
}

export function parseManifestAck(value: unknown): ManifestAck {
  return parseValue(manifestAckSchema, value, 'manifest_ack');
}

export function parseInvoke(value: unknown): InvokeMessage {
  return parseValue(invokeSchema, value, 'invoke');
}

export function parseResult(value: unknown): ResultMessage {
  return parseValue(z.union([resultOkSchema, resultErrorSchema]), value, 'result');
}

export function parseSubscribe(value: unknown): SubscribeMessage {
  return parseValue(subscribeSchema, value, 'subscribe');
}

export function parseSubscriptionAck(value: unknown): SubscriptionAck {
  return parseValue(subscriptionAckSchema, value, 'subscription_ack');
}

export function parseEvent(value: unknown): EventMessage {
  return parseValue(eventSchema, value, 'event');
}

export function parseCancel(value: unknown): CancelMessage {
  return parseValue(cancelSchema, value, 'cancel');
}

export function parsePing(value: unknown): PingMessage {
  return parseValue(pingSchema, value, 'ping');
}

export function parsePong(value: unknown): PongMessage {
  return parseValue(pongSchema, value, 'pong');
}

export function parseProviderError(value: unknown): ProviderErrorMessage {
  return parseValue(providerErrorSchema, value, 'error');
}

export function parseProviderMessage(value: unknown): ProviderMessage {
  return parseValue(providerMessageSchema, value, 'provider_message');
}

export function parseJsonText(text: string, maxBytes: number, field: string): unknown {
  if (typeof text !== 'string') throw new ProviderProtocolError(`invalid ${field}`);
  if (text.length > maxBytes || utf8ByteLength(text) > maxBytes) {
    throw new ProviderProtocolError(`${field} is too large`, PROVIDER_CLOSE_CODES.SIZE);
  }
  try {
    return JSON.parse(text);
  } catch {
    throw new ProviderProtocolError(`invalid ${field} JSON`);
  }
}

export function parseServerHelloText(text: string): ServerHello {
  return parseServerHello(parseJsonText(text, MAX_HANDSHAKE_FRAME_BYTES, 'server_hello'));
}

export function parseClientHelloText(text: string): ClientHello {
  return parseClientHello(parseJsonText(text, MAX_HANDSHAKE_FRAME_BYTES, 'client_hello'));
}

export function parseFrameText(text: string): EncryptedFrame {
  return parseFrame(parseJsonText(text, MAX_OUTER_FRAME_BYTES, 'frame'));
}

export function parseProviderMessageText(text: string): ProviderMessage {
  return parseProviderMessage(parseJsonText(text, MAX_INNER_PLAINTEXT_BYTES, 'provider_message'));
}

export function serializeMessage(message: ProviderMessage): string {
  return JSON.stringify(parseProviderMessage(message));
}

export const providerSystemInfoSchema = z
  .object({
    browser_family: nonEmptyString(64),
    browser_version: nonEmptyString(64),
    extension_version: nonEmptyString(32),
    capabilities: z.array(capabilityIdSchema).max(32),
  })
  .strict();

export const providerTabSchema = z
  .object({
    tab_id: nonEmptyString(128),
    window_id: integer(0, Number.MAX_SAFE_INTEGER),
    title: maxString(1024),
    url: maxString(8192),
    active: z.boolean(),
  })
  .strict();

export const providerTabsSchema = z.array(providerTabSchema).max(200);

export type ProviderSystemInfo = z.infer<typeof providerSystemInfoSchema>;
export type ProviderTab = z.infer<typeof providerTabSchema>;

export function parseProviderSystemInfo(value: unknown): ProviderSystemInfo {
  return parseValue(providerSystemInfoSchema, value, 'system.info result');
}

export function parseProviderTabs(value: unknown): ProviderTab[] {
  return parseValue(providerTabsSchema, value, 'tabs.read result');
}
