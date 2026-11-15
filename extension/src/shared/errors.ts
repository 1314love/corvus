/** Stable tool error contract shared by every extension context. */

export const ERROR_CODES = [
  'BAD_ARGS',
  'NOT_FOUND',
  'NOT_INTERACTABLE',
  'TIMEOUT',
  'NAVIGATION_LOST',
  'CAPABILITY_UNAVAILABLE',
  'BROWSER_GONE',
  'TRANSPORT_DOWN',
  'CANCELLED',
  'INTERNAL',
  'POLICY_DENIED',
] as const;

export type ToolErrorCode = (typeof ERROR_CODES)[number];

export const RETRYABLE: Readonly<Record<ToolErrorCode, boolean>> = Object.freeze({
  BAD_ARGS: false,
  NOT_FOUND: true,
  NOT_INTERACTABLE: true,
  TIMEOUT: true,
  NAVIGATION_LOST: true,
  CAPABILITY_UNAVAILABLE: false,
  BROWSER_GONE: false,
  TRANSPORT_DOWN: true,
  CANCELLED: false,
  INTERNAL: false,
  POLICY_DENIED: false,
});

export interface ToolErrorPayload {
  code: ToolErrorCode;
  message: string;
  retryable: boolean;
  details?: unknown;
  trace_id?: string;
}

export class ToolError extends Error {
  readonly code: ToolErrorCode;
  readonly details: unknown;

  constructor(code: string, message: string, details?: unknown) {
    super(message);
    this.name = 'ToolError';
    this.code = normalizeToolErrorCode(code);
    this.details = details === undefined ? null : details;
  }

  toPayload(traceId?: string | null): ToolErrorPayload {
    const payload: ToolErrorPayload = {
      code: this.code,
      message: this.message,
      retryable: RETRYABLE[this.code] === true,
    };
    if (this.details !== null && this.details !== undefined) payload.details = this.details;
    if (traceId) payload.trace_id = traceId;
    return payload;
  }
}

export function describeError(error: unknown): string {
  if (error instanceof Error) return error.message || error.name;
  if (typeof error === 'string') return error;
  if (error && typeof error === 'object' && 'message' in error) return String((error as { message: unknown }).message);
  return String(error);
}

export function toToolErrorPayload(error: unknown, traceId?: string | null): ToolErrorPayload {
  if (error instanceof ToolError) return error.toPayload(traceId);
  const parsed = parseToolErrorPayload(error);
  if (parsed) {
    if (traceId) parsed.trace_id = traceId;
    return parsed;
  }
  const payload: ToolErrorPayload = {
    code: 'INTERNAL',
    message: describeError(error),
    retryable: false,
  };
  if (traceId) payload.trace_id = traceId;
  return payload;
}

/** Map an unknown wire/debug code to a known code; unknown values degrade to INTERNAL. */
export function normalizeToolErrorCode(value: unknown): ToolErrorCode {
  return typeof value === 'string' && (ERROR_CODES as readonly string[]).includes(value)
    ? (value as ToolErrorCode)
    : 'INTERNAL';
}

/**
 * Parse a daemon/content error payload without trusting its retryable flag.
 *
 * Accepts either the bare error body, a JSON string of it, or the REST
 * envelope { status: 403, error: { ... } }. Unknown codes degrade to INTERNAL,
 * and retryability always comes from the local code table.
 */
export function parseToolErrorPayload(value: unknown): ToolErrorPayload | null {
  let candidate = value;
  if (typeof candidate === 'string') {
    const text = candidate.trim();
    if (!text.startsWith('{')) return null;
    try {
      candidate = JSON.parse(text);
    } catch {
      return null;
    }
  }
  if (!candidate || typeof candidate !== 'object') return null;

  const record = candidate as Record<string, unknown>;
  const nested = record.error;
  if (nested && typeof nested === 'object') candidate = nested;

  const parsed = candidate as Record<string, unknown>;
  if (typeof parsed.code !== 'string') return null;
  const code = normalizeToolErrorCode(parsed.code);
  const payload: ToolErrorPayload = {
    code,
    message: typeof parsed.message === 'string' && parsed.message.trim() ? parsed.message : 'tool call failed',
    retryable: RETRYABLE[code] === true,
  };
  if (parsed.details !== undefined) payload.details = parsed.details;
  if (typeof parsed.trace_id === 'string' && parsed.trace_id) payload.trace_id = parsed.trace_id;
  return payload;
}

/**
 * Extract a display-safe policy denial reason. Only a short reason token is
 * accepted; paths, URLs and policy identifiers are omitted so the UI cannot
 * expose guardrail configuration.
 */
export function sanitizePolicyReason(value: unknown): string | null {
  if (typeof value !== 'string') return null;
  const text = value.replace(/\s+/g, ' ').trim();
  if (!text || /[\\/:]/.test(text)) return null;
  return text.length > 120 ? text.slice(0, 119) + '\u2026' : text;
}

export function policyDeniedReasonFromDetails(details: unknown): string | null {
  if (!details || typeof details !== 'object') return null;
  return sanitizePolicyReason((details as Record<string, unknown>).reason);
}
