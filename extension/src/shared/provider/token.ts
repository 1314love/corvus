import { decodeBase64Url, encodeBase64Url } from './protocol';

export const PAIRING_TOKEN_BYTES = 32;

export function normalizePairingToken(value: unknown): string | null {
  if (typeof value !== 'string') return null;
  const text = value.trim();
  if (!/^[A-Za-z0-9_-]{43}$/.test(text)) return null;
  try {
    const bytes = decodeBase64Url(text);
    if (bytes.length !== PAIRING_TOKEN_BYTES) return null;
    return encodeBase64Url(bytes);
  } catch {
    return null;
  }
}
