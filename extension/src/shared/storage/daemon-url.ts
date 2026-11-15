/** Shared daemon WebSocket URL validation and storage. */

import { hasStorage, readStorageValue, removeStorageValue, writeStorageValue } from './adapter';

export const DEFAULT_DAEMON_URL = 'ws://127.0.0.1:7330';
export const DAEMON_URL_STORAGE_KEY = 'corvus.daemonUrl';

export interface DaemonUrlValidation {
  ok: boolean;
  value?: string;
  error?: string;
}

export class DaemonConfigurationError extends Error {
  readonly code = 'INVALID_DAEMON_URL';
  constructor(message: string) {
    super(message);
    this.name = 'DaemonConfigurationError';
  }
}

export function validateDaemonUrl(value: unknown): DaemonUrlValidation {
  if (typeof value !== 'string') return { ok: false, error: 'Enter a WebSocket URL.' };

  const text = value.trim();
  if (!text) return { ok: false, error: 'Enter a WebSocket URL.' };
  if (text.indexOf('?') !== -1 || text.indexOf('#') !== -1) {
    return { ok: false, error: 'Remove the query string and fragment; only ws://host[:port] is supported.' };
  }

  const authority = text.match(/^[a-z][a-z0-9+.-]*:\/\/([^/?#]*)/i);
  if (authority && authority[1].endsWith(':')) {
    return { ok: false, error: 'Enter a port number after the colon, or remove the colon.' };
  }

  let url: URL;
  try {
    url = new URL(text);
  } catch {
    return { ok: false, error: 'Enter a valid WebSocket URL.' };
  }

  if (url.protocol !== 'ws:' && url.protocol !== 'wss:') return { ok: false, error: 'Use ws:// or wss://.' };
  if (!url.hostname) return { ok: false, error: 'The URL must include a host.' };
  if (url.username || url.password) return { ok: false, error: 'Remove the username and password from the URL.' };
  if (url.pathname !== '/') return { ok: false, error: 'Use ws://host[:port]; paths are not supported.' };
  if (url.search || url.hash) {
    return { ok: false, error: 'Remove the query string and fragment; only ws://host[:port] is supported.' };
  }

  return { ok: true, value: url.protocol + '//' + url.host };
}

export function normalizeDaemonUrl(value: unknown): string | null {
  const result = validateDaemonUrl(value);
  return result.ok && result.value ? result.value : null;
}

function requireValidDaemonUrl(value: unknown): string {
  const result = validateDaemonUrl(value);
  if (!result.ok || !result.value) throw new DaemonConfigurationError(result.error || 'Invalid daemon URL');
  return result.value;
}

export async function readDaemonUrl(): Promise<string> {
  if (!hasStorage()) return DEFAULT_DAEMON_URL;
  const stored = await readStorageValue(DAEMON_URL_STORAGE_KEY);
  return normalizeDaemonUrl(stored) || DEFAULT_DAEMON_URL;
}

export async function writeDaemonUrl(value: unknown): Promise<string> {
  const normalized = requireValidDaemonUrl(value);
  await writeStorageValue(DAEMON_URL_STORAGE_KEY, normalized);
  return normalized;
}

export async function resetDaemonUrl(): Promise<string> {
  await removeStorageValue(DAEMON_URL_STORAGE_KEY);
  return DEFAULT_DAEMON_URL;
}
