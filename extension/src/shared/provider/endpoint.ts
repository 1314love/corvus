import { DEFAULT_DAEMON_URL } from '../storage/daemon-url';

export const PROVIDER_ENDPOINT_PATH = '/provider/v2';
export const PAIRING_TOKEN_STORAGE_KEY = 'provider.pairingToken';
export const DAEMON_URL_STORAGE_KEY = 'corvus.daemonUrl';

export function isLoopbackHost(hostname: string): boolean {
  const host = hostname.toLowerCase();
  return host === '127.0.0.1' || host === 'localhost' || host === '[::1]' || host === '::1';
}

export function providerEndpointFromBase(base: unknown): string | null {
  if (typeof base !== 'string') return null;
  const text = base.trim();
  if (!text) return null;
  let url: URL;
  try {
    url = new URL(text);
  } catch {
    return null;
  }
  if (url.protocol !== 'ws:') return null;
  if (!isLoopbackHost(url.hostname)) return null;
  if (url.username || url.password) return null;
  if (url.pathname !== '/' || url.search || url.hash) return null;
  return url.protocol + '//' + url.host + PROVIDER_ENDPOINT_PATH;
}

export function providerBaseFromStorageValue(value: unknown): string {
  if (typeof value === 'string' && providerEndpointFromBase(value)) return value.trim();
  return DEFAULT_DAEMON_URL;
}
