import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  applyLocale,
  changeLocale,
  detectLocale,
  i18n,
  initializeI18n,
  LOCALE_STORAGE_KEY,
  normalizeLocale,
  parseLocale,
  readStoredLocale,
} from '../../src/ui/shared/i18n';

function installStorage(initial: Record<string, unknown> = {}): {
  values: Record<string, unknown>;
  set: ReturnType<typeof vi.fn>;
} {
  const values: Record<string, unknown> = { ...initial };
  const set = vi.fn(async (items: Record<string, unknown>) => {
    Object.assign(values, items);
  });
  const get = vi.fn(async (key: string) => (key in values ? { [key]: values[key] } : {}));
  const remove = vi.fn(async (key: string) => {
    delete values[key];
  });
  vi.stubGlobal('chrome', { storage: { local: { get, set, remove } } });
  return { values, set };
}

afterEach(() => {
  applyLocale('en');
  vi.unstubAllGlobals();
});

describe('i18n locale helpers', () => {
  it('parses Chinese and English locales case-insensitively', () => {
    expect(parseLocale('zh')).toBe('zh-CN');
    expect(parseLocale('zh-TW')).toBe('zh-CN');
    expect(parseLocale('ZH-Hans')).toBe('zh-CN');
    expect(parseLocale('en-US')).toBe('en');
    expect(parseLocale('EN')).toBe('en');
  });

  it('rejects unsupported and non-string locale values', () => {
    expect(parseLocale('fr-FR')).toBeNull();
    expect(parseLocale('')).toBeNull();
    expect(parseLocale(null)).toBeNull();
    expect(parseLocale(42)).toBeNull();
    expect(normalizeLocale('fr-FR')).toBe('en');
    expect(normalizeLocale(undefined)).toBe('en');
  });

  it('detects the browser UI language through chrome.i18n first', () => {
    vi.stubGlobal('chrome', { i18n: { getUILanguage: () => 'zh-TW' } });
    expect(detectLocale()).toBe('zh-CN');
    vi.stubGlobal('chrome', { i18n: { getUILanguage: () => 'en-GB' } });
    expect(detectLocale()).toBe('en');
  });

  it('falls back to navigator.language when chrome.i18n is absent', () => {
    vi.stubGlobal('chrome', undefined);
    vi.stubGlobal('navigator', { language: 'zh-Hans-CN' });
    expect(detectLocale()).toBe('zh-CN');
  });

  it('reads only a supported persisted locale', async () => {
    installStorage({ [LOCALE_STORAGE_KEY]: 'zh-CN' });
    expect(await readStoredLocale()).toBe('zh-CN');

    installStorage({ [LOCALE_STORAGE_KEY]: 'fr-FR' });
    expect(await readStoredLocale()).toBeNull();

    vi.stubGlobal('chrome', undefined);
    expect(await readStoredLocale()).toBeNull();
  });

  it('initializes from stored locale and applies document.lang', async () => {
    installStorage({ [LOCALE_STORAGE_KEY]: 'zh-CN' });
    await expect(initializeI18n()).resolves.toBe('zh-CN');
    expect(document.documentElement.lang).toBe('zh-CN');
    expect((i18n.global.locale as unknown as { value: string }).value).toBe('zh-CN');
  });

  it('initializes from browser language when nothing valid is stored', async () => {
    installStorage({ [LOCALE_STORAGE_KEY]: 'fr-FR' });
    vi.stubGlobal('navigator', { language: 'en-US' });
    await expect(initializeI18n()).resolves.toBe('en');
    expect(document.documentElement.lang).toBe('en');
  });

  it('changes locale reactively and persists the choice', async () => {
    const storage = installStorage();
    await changeLocale('zh-CN');
    expect(document.documentElement.lang).toBe('zh-CN');
    expect(storage.values[LOCALE_STORAGE_KEY]).toBe('zh-CN');

    await changeLocale('en');
    expect(document.documentElement.lang).toBe('en');
    expect(storage.values[LOCALE_STORAGE_KEY]).toBe('en');
  });

  it('applies the in-memory locale when storage is unavailable', async () => {
    vi.stubGlobal('chrome', undefined);
    await expect(changeLocale('zh-CN')).resolves.toBeUndefined();
    expect(document.documentElement.lang).toBe('zh-CN');
  });
});
