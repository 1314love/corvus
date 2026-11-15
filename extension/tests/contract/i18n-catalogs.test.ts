import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import en from '../../src/ui/shared/i18n/locales/en';
import zhCN from '../../src/ui/shared/i18n/locales/zh-CN';

type Catalog = Record<string, unknown>;

function flatten(catalog: Catalog, prefix = ''): Record<string, string> {
  const result: Record<string, string> = {};
  for (const [key, value] of Object.entries(catalog)) {
    const path = prefix ? prefix + '.' + key : key;
    if (typeof value === 'string') result[path] = value;
    else if (value && typeof value === 'object') Object.assign(result, flatten(value as Catalog, path));
  }
  return result;
}

const enFlat = flatten(en);
const zhFlat = flatten(zhCN);

describe('i18n catalogs', () => {
  it('has exactly the same flattened keys in English and Chinese', () => {
    expect(Object.keys(zhFlat).sort()).toEqual(Object.keys(enFlat).sort());
  });

  it('has non-empty text for every key in both locales', () => {
    for (const [key, value] of Object.entries(enFlat)) expect(value.trim(), 'en ' + key).not.toBe('');
    for (const [key, value] of Object.entries(zhFlat)) expect(value.trim(), 'zh-CN ' + key).not.toBe('');
  });

  it('contains the critical keys used by the popup, options and view model', () => {
    const required = [
      'app.title',
      'app.popupTitle',
      'app.optionsTitle',
      'app.version',
      'common.save',
      'common.resetToDefault',
      'common.detectNow',
      'common.english',
      'common.chinese',
      'options.daemonConnection',
      'options.browserCdpEndpoint',
      'options.status',
      'status.connected',
      'family.match',
      'family.mismatch',
      'source.auto',
      'notice.daemonSaved',
      'notice.cdpSaved',
      'validation.daemon',
      'validation.cdp',
    ];
    for (const key of required) expect(enFlat, 'en ' + key).toHaveProperty(key);
    for (const key of required) expect(zhFlat, 'zh-CN ' + key).toHaveProperty(key);
  });

  it('uses only catalog keys referenced by popup/options sources', () => {
    const files = ['src/ui/popup/App.vue', 'src/ui/options/App.vue', 'src/ui/shared/viewmodels/session.ts'];
    for (const file of files) {
      const source = readFileSync(resolve(process.cwd(), file), 'utf8');
      for (const match of source.matchAll(/\bt\(\s*'([^']+)'/g)) {
        const key = match[1];
        if (!key || key.endsWith('.')) continue;
        expect(enFlat, file + ' -> ' + key).toHaveProperty(key);
      }
    }
  });

  it('declares every manifest placeholder in both locale files', () => {
    const manifest = JSON.parse(readFileSync(resolve(process.cwd(), 'manifest.json'), 'utf8')) as Record<
      string,
      unknown
    >;
    const action = (manifest.action || {}) as Record<string, unknown>;
    const references = [manifest.name, manifest.description, action.default_title]
      .map((value) => (typeof value === 'string' ? /^__MSG_(\w+)__$/.exec(value) : null))
      .filter((match): match is RegExpExecArray => match !== null)
      .map((match) => match[1]);
    expect(references.length).toBe(3);

    for (const locale of ['en', 'zh_CN']) {
      const messages = JSON.parse(
        readFileSync(resolve(process.cwd(), '_locales', locale, 'messages.json'), 'utf8'),
      ) as Record<string, { message?: string }>;
      for (const reference of references) {
        expect(messages[reference], locale + ' ' + reference).toBeDefined();
        expect(messages[reference]?.message?.trim()).toBeTruthy();
      }
    }
  });
});
