import { describe, expect, it } from 'vitest';
import {
  ERROR_CODES,
  RETRYABLE,
  ToolError,
  normalizeToolErrorCode,
  parseToolErrorPayload,
  policyDeniedReasonFromDetails,
  sanitizePolicyReason,
  toToolErrorPayload,
} from '../../src/shared/errors';

describe('tool error taxonomy', () => {
  it('has eleven codes with POLICY_DENIED last', () => {
    expect(ERROR_CODES).toHaveLength(11);
    expect(ERROR_CODES[10]).toBe('POLICY_DENIED');
  });

  it('marks POLICY_DENIED non-retryable', () => {
    expect(RETRYABLE.POLICY_DENIED).toBe(false);
  });

  it('serializes POLICY_DENIED and survives the JSON wire boundary', () => {
    const error = new ToolError('POLICY_DENIED', 'Blocked by daemon policy', {
      reason: 'deny_list',
      tool: 'browser_click',
      policy: 'guardrails.json',
    });
    const payload = error.toPayload('trace-1');
    expect(payload).toMatchObject({
      code: 'POLICY_DENIED',
      message: 'Blocked by daemon policy',
      retryable: false,
      trace_id: 'trace-1',
      details: { reason: 'deny_list', tool: 'browser_click', policy: 'guardrails.json' },
    });

    const wire = JSON.parse(JSON.stringify(payload));
    expect(parseToolErrorPayload(wire)).toMatchObject({
      code: 'POLICY_DENIED',
      retryable: false,
      details: { reason: 'deny_list', tool: 'browser_click', policy: 'guardrails.json' },
    });
  });

  it('parses a daemon-shaped 403 POLICY_DENIED envelope and recomputes retryable', () => {
    const payload = parseToolErrorPayload({
      status: 403,
      error: {
        code: 'POLICY_DENIED',
        message: 'Blocked by daemon policy',
        retryable: true,
        details: { reason: 'allowlist', url: 'https://example.test/private', policy: '/etc/corvus/policy.json' },
      },
    });
    expect(payload).toMatchObject({
      code: 'POLICY_DENIED',
      message: 'Blocked by daemon policy',
      retryable: false,
      details: {
        reason: 'allowlist',
        url: 'https://example.test/private',
        policy: '/etc/corvus/policy.json',
      },
    });
  });

  it('parses JSON strings and preserves details', () => {
    const payload = parseToolErrorPayload(
      JSON.stringify({ code: 'POLICY_DENIED', message: 'denied', details: { reason: 'private_network' } }),
    );
    expect(payload?.code).toBe('POLICY_DENIED');
    expect(payload?.details).toEqual({ reason: 'private_network' });
  });

  it('degrades unknown or malformed codes to INTERNAL', () => {
    expect(normalizeToolErrorCode('NOT_REAL')).toBe('INTERNAL');
    expect(new ToolError('NOT_REAL', 'boom').code).toBe('INTERNAL');
    expect(parseToolErrorPayload({ code: 'NOT_REAL', message: 'boom' })).toMatchObject({
      code: 'INTERNAL',
      retryable: false,
    });
    expect(parseToolErrorPayload({ message: 'missing code' })).toBeNull();
    expect(toToolErrorPayload({ code: 'POLICY_DENIED', message: 'denied' }).code).toBe('POLICY_DENIED');
  });

  it('keeps only a short safe reason for display', () => {
    expect(policyDeniedReasonFromDetails({ reason: 'deny_list', policy: '/etc/corvus/policy.json' })).toBe('deny_list');
    expect(policyDeniedReasonFromDetails({ reason: '/etc/corvus/policy.json' })).toBeNull();
    expect(policyDeniedReasonFromDetails({ reason: 'https://internal/policy' })).toBeNull();
    expect(sanitizePolicyReason('  deny   list  ')).toBe('deny list');
    expect(sanitizePolicyReason('x'.repeat(140))).toHaveLength(120);
  });
});
