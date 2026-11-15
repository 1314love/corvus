/**
 * Shared popup/options i18n.
 *
 * Locale is a UI presentation preference, not runtime bridge state. It is read
 * before mount so the first paint uses the stored language, and explicit choices
 * are persisted under corvus.locale. The service worker remains the owner of
 * daemon/CDP settings and all bridge state.
 */

import { createI18n } from 'vue-i18n';
import { readStorageValue, writeStorageValue } from '../../../shared/storage/adapter';
import en from './locales/en';
import zhCN from './locales/zh-CN';

export type LocaleCode = 'en' | 'zh-CN';

export const SUPPORTED_LOCALES: readonly LocaleCode[] = ['en', 'zh-CN'];
export const LOCALE_STORAGE_KEY = 'corvus.locale';

/** Parse any stored/browser locale. Unknown values return null so detection can run. */
export function parseLocale(value: unknown): LocaleCode | null {
  if (typeof value !== 'string') return null;
  const normalized = value.trim().toLowerCase();
  if (!normalized) return null;
  if (normalized.startsWith('zh')) return 'zh-CN';
  if (normalized.startsWith('en')) return 'en';
  return null;
}

/** Normalize a browser locale to a supported UI locale; unsupported languages use English. */
export function normalizeLocale(value: unknown): LocaleCode {
  return parseLocale(value) ?? 'en';
}

function browserLanguage(): string {
  try {
    if (typeof chrome !== 'undefined' && chrome.i18n && typeof chrome.i18n.getUILanguage === 'function') {
      const uiLanguage = chrome.i18n.getUILanguage();
      if (uiLanguage) return uiLanguage;
    }
  } catch {
    // chrome.i18n is absent in some test and preview contexts.
  }
  try {
    if (typeof navigator !== 'undefined' && typeof navigator.language === 'string') return navigator.language;
  } catch {
    // Ignore an unavailable navigator and fall through to English.
  }
  return '';
}

/** Detect the initial locale from the browser UI language. */
export function detectLocale(): LocaleCode {
  return normalizeLocale(browserLanguage());
}

export const i18n = createI18n({
  legacy: false,
  locale: detectLocale(),
  fallbackLocale: 'en',
  messages: {
    en,
    'zh-CN': zhCN,
  } as any,
  missingWarn: false,
  fallbackWarn: false,
});

/** Apply a locale to the reactive i18n instance and the document language. */
export function applyLocale(locale: LocaleCode): void {
  (i18n.global.locale as unknown as { value: LocaleCode }).value = locale;
  if (typeof document !== 'undefined') document.documentElement.lang = locale;
}

/** Read an explicitly persisted locale, if any. */
export async function readStoredLocale(): Promise<LocaleCode | null> {
  return parseLocale(await readStorageValue(LOCALE_STORAGE_KEY));
}

/**
 * Resolve and apply the startup locale: explicit storage first, then the browser
 * UI language. Called once before the Vue app mounts.
 */
export async function initializeI18n(): Promise<LocaleCode> {
  const stored = await readStoredLocale();
  const locale = stored ?? detectLocale();
  applyLocale(locale);
  return locale;
}

/**
 * Switch the UI language without a reload and persist the explicit choice.
 * Storage may be unavailable in preview/test contexts; the in-memory switch is
 * still applied and persistence failure is deliberately non-fatal.
 */
export async function changeLocale(locale: LocaleCode): Promise<void> {
  applyLocale(locale);
  try {
    await writeStorageValue(LOCALE_STORAGE_KEY, locale);
  } catch {
    // chrome.storage.local is unavailable: keep the in-memory choice.
  }
}
