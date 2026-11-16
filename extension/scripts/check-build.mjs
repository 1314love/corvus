import { readFileSync, readdirSync, statSync } from 'node:fs';
import { createHash } from 'node:crypto';
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
  if (!Array.isArray(manifest.permissions) || manifest.permissions.length !== 11)
    failures.push('permissions must contain exactly 11 entries');
  const expectedPermissions = [
    'tabs',
    'activeTab',
    'scripting',
    'storage',
    'debugger',
    'webRequest',
    'downloads',
    'alarms',
    'offscreen',
    'unlimitedStorage',
    'tabGroups',
  ];
  for (const permission of expectedPermissions) {
    if (!manifest.permissions.includes(permission)) failures.push('missing permission ' + permission);
  }
  if (
    !manifest.background ||
    manifest.background.service_worker !== 'background.js' ||
    manifest.background.type !== 'module'
  ) {
    failures.push('background must be an ESM service worker');
  }
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
const background = (() => {
  try {
    return readFileSync(resolve(out, 'background.js'), 'utf8');
  } catch {
    return '';
  }
})();
if (!/^import |^const |^var |^function |^class /m.test(background))
  failures.push('background.js does not look like an ES module');
const content = (() => {
  try {
    return readFileSync(resolve(out, 'content.js'), 'utf8');
  } catch {
    return '';
  }
})();
if (/^import\s/m.test(content)) failures.push('content.js must be standalone IIFE output');

const extensionTools = [
  'browser_navigate',
  'page_snapshot',
  'browser_click',
  'browser_type',
  'page_text',
  'page_html',
  'page_find',
  'browser_hover',
  'browser_press',
  'browser_scroll',
  'browser_select',
  'browser_eval',
  'browser_wait',
  'browser_history',
  'tab_list',
  'tab_new',
  'tab_close',
  'tab_switch',
  'net_capture',
  'net_log',
  'net_clear',
  'net_intercept',
  'download_list',
  'download_wait',
  'upload_file',
  'browser_screenshot',
  'page_pdf',
  'page_links',
  'page_forms',
  'page_accessibility',
  'browser_drag',
  'browser_focus',
  'browser_highlight',
  'tab_group',
  'tab_duplicate',
  'window_list',
  'window_new',
  'cookie_set',
  'cookie_clear',
  'storage_get',
  'storage_set',
  'storage_clear',
];
if (extensionTools.length !== 42)
  failures.push('expected exactly 42 extension handlers, found ' + extensionTools.length);
if (new Set(extensionTools).size !== extensionTools.length) failures.push('extension handler list contains duplicates');
for (const name of extensionTools) {
  if (!background.includes(name)) failures.push('background.js is missing tool ' + name);
}

const expectedSnapshotHash = '0763eee9256bae1942c1480317fc6a80375257d44b3674261b689ae1f3844114';
const expectedToolCount = 62;
const snapshot = resolve(root, '..', 'tests', 'conformance', 'schema-snapshot.v1.7.0.json');
const snapshotBytes = readFileSync(snapshot);
const snapshotHash = createHash('sha256').update(snapshotBytes).digest('hex');
if (snapshotHash !== expectedSnapshotHash)
  failures.push('schema snapshot hash is ' + snapshotHash + ', expected ' + expectedSnapshotHash);
let snapshotTools = [];
try {
  snapshotTools = JSON.parse(snapshotBytes.toString('utf8'));
} catch (error) {
  failures.push('schema snapshot is not valid JSON: ' + error.message);
}
if (!Array.isArray(snapshotTools)) {
  failures.push('schema snapshot must be an array');
  snapshotTools = [];
} else if (snapshotTools.length !== expectedToolCount) {
  failures.push('schema snapshot must contain exactly ' + expectedToolCount + ' tools, found ' + snapshotTools.length);
}
const snapshotNames = snapshotTools.map((tool) => tool && tool.name);
if (snapshotNames.some((name) => typeof name !== 'string' || name === ''))
  failures.push('schema snapshot contains an invalid tool name');
for (const name of extensionTools) {
  if (!snapshotNames.includes(name)) failures.push('extension handler is not in the tool catalogue: ' + name);
}
const generated = readFileSync(resolve(root, 'src', 'shared', 'contracts', 'tools.generated.ts'), 'utf8');
if (!generated.includes('schema-snapshot.v1.7.0.json')) failures.push('generated contracts do not name the snapshot');
const generatedMatch = generated.match(/export const TOOL_NAMES = \[([\s\S]*?)\] as const;/);
let generatedNames = [];
if (!generatedMatch) {
  failures.push('generated contracts do not declare TOOL_NAMES');
} else {
  try {
    generatedNames = JSON.parse('[' + generatedMatch[1] + ']');
  } catch (error) {
    failures.push('generated TOOL_NAMES is not valid JSON: ' + error.message);
  }
}
if (generatedNames.length !== expectedToolCount)
  failures.push(
    'generated contracts must contain exactly ' + expectedToolCount + ' tools, found ' + generatedNames.length,
  );
else if (JSON.stringify(generatedNames) !== JSON.stringify(snapshotNames))
  failures.push('generated contracts do not match the schema snapshot tool names and order');

const permissionCount = manifest && Array.isArray(manifest.permissions) ? manifest.permissions.length : 0;
const sizes = {};
for (const name of ['background.js', 'content.js']) {
  try {
    sizes[name] = statSync(resolve(out, name)).size;
  } catch {
    sizes[name] = 0;
  }
}
const assets = files
  .filter((file) => file.includes('assets'))
  .map((file) => ({ path: relative(out, file), bytes: statSync(file).size }));
if (failures.length) {
  console.error(JSON.stringify({ ok: false, failures, sizes, snapshotHash }, null, 2));
  process.exit(1);
}
process.stdout.write(
  JSON.stringify(
    {
      ok: true,
      files: files.length,
      tools: snapshotNames.length,
      handlers: extensionTools.length,
      permissions: permissionCount,
      sizes,
      assets,
      snapshotHash,
    },
    null,
    2,
  ) + '\n',
);
