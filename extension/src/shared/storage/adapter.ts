/**
 * Storage adapter over chrome.storage.local.
 *
 * Background is the only writer. Offscreen and content contexts may not expose
 * chrome.storage at all, so every accessor degrades to an absent area instead of
 * throwing during boot.
 */

export interface StorageAreaLike {
  get(keys?: string | string[] | Record<string, unknown> | null): Promise<Record<string, unknown>>;
  set(items: Record<string, unknown>): Promise<void>;
  remove(keys: string | string[]): Promise<void>;
}

export function getStorageArea(): StorageAreaLike | null {
  if (typeof chrome === 'undefined' || !chrome.storage || !chrome.storage.local) return null;
  return chrome.storage.local as unknown as StorageAreaLike;
}

export function hasStorage(): boolean {
  return getStorageArea() !== null;
}

export async function readStorageValue(key: string): Promise<unknown> {
  const area = getStorageArea();
  if (!area) return undefined;
  try {
    const stored = await area.get(key);
    return stored ? stored[key] : undefined;
  } catch {
    return undefined;
  }
}

export async function writeStorageValue(key: string, value: unknown): Promise<void> {
  const area = getStorageArea();
  if (!area) throw new Error('chrome.storage.local is unavailable');
  await area.set({ [key]: value });
}

export async function removeStorageValue(key: string): Promise<void> {
  const area = getStorageArea();
  if (!area) throw new Error('chrome.storage.local is unavailable');
  await area.remove(key);
}
