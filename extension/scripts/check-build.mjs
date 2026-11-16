import { readFileSync, readdirSync, statSync } from 'node:fs';
import { extname, relative, resolve } from 'node:path';

const root = process.cwd();
const out = resolve(root, 'build', 'extension');
const localeNames = ['en', 'zh_CN'];
const required = [
  'manifest.json',
  'background.js',
  'content.js',
  'popup.html',
  'options.html',
  'offscreen.html',
  '_locales/en/messages.json',
  '_locales/zh_CN/messages.json',
];
const expectedPermissions = ['tabs', 'activeTab', 'scripting', 'storage', 'offscreen'];
const providerActions = [
  'system.info.info',
  'tabs.list',
  'tabs.open',
  'tabs.close',
  'page.navigate',
  'page.read',
  'page.snapshot',
  'input.click',
  'input.type',
  'capture.viewport',
];
const failures = [];

function walk(dir) {
  const files = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = resolve(dir, entry.name);
    if (entry.isDirectory()) files.push(...walk(path));
    else files.push(path);
  }
  return files;
}

for (const name of required) {
  try {
    const info = statSync(resolve(out, name));
    if (!info.isFile() || info.size === 0) failures.push(name + ' is empty');
  } catch {
    failures.push('missing ' + name);
  }
}

let manifest = null;
let manifestText = '';
try {
  manifestText = readFileSync(resolve(out, 'manifest.json'), 'utf8');
  manifest = JSON.parse(manifestText);
} catch (error) {
  failures.push('manifest.json is not valid JSON: ' + error.message);
}
if (manifest) {
  if (manifest.manifest_version !== 3) failures.push('manifest_version is not 3');
  if (manifest.version !== '1.7.0') failures.push('manifest version is not 1.7.0');
  if (!Array.isArray(manifest.permissions)) failures.push('permissions must be an array');
  else if (JSON.stringify(manifest.permissions) !== JSON.stringify(expectedPermissions)) {
    failures.push(
      'permissions must be exactly ' + expectedPermissions.join(',') + '; found ' + manifest.permissions.join(','),
    );
  }
  if (manifestText.includes('debugger')) failures.push('manifest must not mention debugger');
  if (!manifest.background || manifest.background.service_worker !== 'background.js')
    failures.push('background must be background.js');
  if (manifest.background && manifest.background.type !== 'module') failures.push('background must be an ES module');
  if (!manifest.host_permissions || manifest.host_permissions[0] !== '<all_urls>')
    failures.push('host_permissions must include <all_urls>');
  if (!manifest.content_scripts || manifest.content_scripts[0].js[0] !== 'content.js')
    failures.push('content script must be content.js');
  if (!manifest.content_scripts || manifest.content_scripts[0].all_frames !== false)
    failures.push('content script all_frames must be false');
  if (!manifest.options_ui || manifest.options_ui.page !== 'options.html')
    failures.push('options_ui must point at options.html');
  if (manifest.default_locale !== 'en') failures.push('default_locale must be en');
  const localizedFields = [
    ['name', manifest.name],
    ['description', manifest.description],
    ['action.default_title', manifest.action && manifest.action.default_title],
  ];
  for (const [field, value] of localizedFields) {
    if (typeof value !== 'string' || !/^__MSG_[A-Za-z0-9_]+__$/.test(value))
      failures.push(field + ' must be an __MSG_*__ placeholder');
  }
}

const localeCatalogs = {};
for (const name of localeNames) {
  try {
    const data = JSON.parse(readFileSync(resolve(out, '_locales', name, 'messages.json'), 'utf8'));
    if (!data || typeof data !== 'object' || Array.isArray(data)) throw new Error('not a JSON object');
    localeCatalogs[name] = data;
  } catch (error) {
    failures.push('_locales/' + name + '/messages.json is missing or invalid: ' + error.message);
  }
}
if (manifest) {
  const placeholders = [...new Set([...manifestText.matchAll(/__MSG_([A-Za-z0-9_]+)__/g)].map((match) => match[1]))];
  if (placeholders.length === 0) failures.push('manifest does not reference any __MSG_*__ placeholders');
  for (const key of placeholders) {
    for (const name of localeNames) {
      const entry = localeCatalogs[name] && localeCatalogs[name][key];
      if (!entry || typeof entry.message !== 'string' || entry.message.trim() === '')
        failures.push('missing message ' + key + ' in _locales/' + name + '/messages.json');
    }
  }
}
const enKeys = localeCatalogs.en ? Object.keys(localeCatalogs.en).sort() : [];
const zhKeys = localeCatalogs.zh_CN ? Object.keys(localeCatalogs.zh_CN).sort() : [];
if (enKeys.length && zhKeys.length && JSON.stringify(enKeys) !== JSON.stringify(zhKeys))
  failures.push('locale catalogs must have the same keys');

const files = (() => {
  try {
    return walk(out);
  } catch {
    return [];
  }
})();
for (const file of files) {
  if (extname(file) === '.map') failures.push('source map shipped: ' + relative(out, file));
}
const read = (name) => {
  try {
    return readFileSync(resolve(out, name), 'utf8');
  } catch {
    return '';
  }
};
const background = read('background.js');
const content = read('content.js');
const jsFiles = files.filter((file) => extname(file) === '.js');
const allJs = jsFiles.map((file) => readFileSync(file, 'utf8')).join('\n');
if (!/^import |^const |^var |^function |^class /m.test(background))
  failures.push('background.js does not look like an ES module');
if (/^import\s/m.test(content)) failures.push('content.js must be standalone IIFE output');
if (/\bdebugger\b/.test(allJs)) failures.push('built JavaScript must not mention debugger');
if (/Page\.captureScreenshot|Accessibility\.getFullAXTree|Network\.setCookie/.test(allJs))
  failures.push('built JavaScript must not use the debugger protocol');
if (/browser_screenshot|page_pdf|schema-snapshot\.v1\.7\.0/.test(allJs))
  failures.push('built JavaScript still contains a legacy 1.x runtime name');
for (const action of providerActions) {
  if (!background.includes(action)) failures.push('background.js is missing provider action ' + action);
}

let offscreenHtml = '';
try {
  offscreenHtml = readFileSync(resolve(out, 'offscreen.html'), 'utf8');
} catch {
  failures.push('missing offscreen.html');
}
const offscreenMatch = /src="\.\/(assets\/[^"]+\.js)"/.exec(offscreenHtml);
let offscreenAsset = '';
let offscreenText = '';
if (!offscreenMatch) failures.push('offscreen.html does not load a bundled script');
else {
  offscreenAsset = offscreenMatch[1];
  try {
    offscreenText = readFileSync(resolve(out, offscreenAsset), 'utf8');
  } catch {
    failures.push('offscreen bundle is missing: ' + offscreenAsset);
  }
}
if (offscreenText && !offscreenText.includes('/provider/v2'))
  failures.push('offscreen bundle does not use the provider endpoint path');

const permissionCount = manifest && Array.isArray(manifest.permissions) ? manifest.permissions.length : 0;
const sizes = {};
for (const name of ['background.js', 'content.js']) sizes[name] = statSync(resolve(out, name)).size;
if (offscreenAsset) sizes[offscreenAsset] = offscreenText.length;
const assets = files
  .filter((file) => file.includes('assets'))
  .map((file) => ({ path: relative(out, file), bytes: statSync(file).size }));

if (failures.length) {
  console.error(JSON.stringify({ ok: false, failures, sizes }, null, 2));
  process.exit(1);
}
process.stdout.write(
  JSON.stringify(
    {
      ok: true,
      files: files.length,
      permissions: permissionCount,
      provider_actions: providerActions.length,
      offscreen_bundle: offscreenAsset,
      sizes,
      assets,
    },
    null,
    2,
  ) + '\n',
);
