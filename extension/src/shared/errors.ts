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
    this.code = ERROR_CODES.includes(code as ToolErrorCode) ? (code as ToolErrorCode) : 'INTERNAL';
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
  const payload: ToolErrorPayload = {
    code: 'INTERNAL',
    message: describeError(error),
    retryable: false,
  };
  if (traceId) payload.trace_id = traceId;
  return payload;
}
