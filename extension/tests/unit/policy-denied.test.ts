import { describe, expect, it } from 'vitest';
import type { PolicyTranslate } from '../../src/ui/shared/policy-denied';
import { localizedErrorText, policyDeniedFromError, policyDeniedText } from '../../src/ui/shared/policy-denied';

const t: PolicyTranslate = (key, params) =>
  key === 'errors.policyDeniedWithReason'
    ? 'Blocked by daemon policy: ' + String(params?.reason)
    : 'Blocked by daemon policy';

describe('policy denial presentation', () => {
  it('formats a reason in English-style templates', () => {
    expect(policyDeniedText({ code: 'POLICY_DENIED', reason: 'deny_list' }, t)).toBe(
      'Blocked by daemon policy: deny_list',
    );
    expect(policyDeniedText({ code: 'POLICY_DENIED', reason: null }, t)).toBe('Blocked by daemon policy');
    expect(policyDeniedText(null, t)).toBeNull();
  });

  it('never renders a policy path or URL as the reason', () => {
    expect(policyDeniedText({ code: 'POLICY_DENIED', reason: 'C:/secret/policy.json' }, t)).toBe(
      'Blocked by daemon policy',
    );
    expect(policyDeniedText({ code: 'POLICY_DENIED', reason: 'https://internal/policy' }, t)).toBe(
      'Blocked by daemon policy',
    );
  });

  it('extracts policy denials from error objects or JSON strings', () => {
    const fromObject = policyDeniedFromError({
      code: 'POLICY_DENIED',
      message: 'denied',
      details: { reason: 'allowlist', policy: 'C:/secret/policy.json' },
    });
    expect(fromObject).toEqual({ code: 'POLICY_DENIED', reason: 'allowlist' });
    expect(
      policyDeniedFromError(JSON.stringify({ code: 'POLICY_DENIED', details: { reason: 'private_network' } })),
    ).toEqual({ code: 'POLICY_DENIED', reason: 'private_network' });
    expect(policyDeniedFromError('plain failure')).toBeNull();
  });

  it('localizes structured error input and leaves non-policy text unchanged', () => {
    expect(localizedErrorText(JSON.stringify({ code: 'POLICY_DENIED', details: { reason: 'deny_list' } }), t)).toBe(
      'Blocked by daemon policy: deny_list',
    );
    expect(localizedErrorText('boom', t)).toBe('boom');
    expect(localizedErrorText('', t)).toBeNull();
  });
});
