/** Presentation helpers for daemon policy denials; never exposes policy paths. */

import type { PolicyDeniedUiState } from '../../shared/contracts/ui';
import { parseToolErrorPayload, policyDeniedReasonFromDetails, sanitizePolicyReason } from '../../shared/errors';

export type PolicyTranslate = (key: string, params?: Record<string, unknown>) => string;

export function policyDeniedText(denied: PolicyDeniedUiState | null | undefined, t: PolicyTranslate): string | null {
  if (!denied || denied.code !== 'POLICY_DENIED') return null;
  const reason = sanitizePolicyReason(denied.reason);
  return reason ? t('errors.policyDeniedWithReason', { reason }) : t('errors.policyDenied');
}

export function policyDeniedFromError(error: unknown): PolicyDeniedUiState | null {
  const payload = parseToolErrorPayload(error);
  if (!payload || payload.code !== 'POLICY_DENIED') return null;
  return { code: 'POLICY_DENIED', reason: policyDeniedReasonFromDetails(payload.details) };
}

export function localizedErrorText(error: unknown, t: PolicyTranslate): string | null {
  if (error === null || error === undefined || error === '') return null;
  const denied = policyDeniedFromError(error);
  if (denied) return policyDeniedText(denied, t);
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message || error.name;
  return String(error);
}
