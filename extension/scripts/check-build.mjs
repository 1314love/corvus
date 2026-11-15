import { readFileSync, readdirSync, statSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { extname, relative, resolve } from 'node:path';

const root = process.cwd();
const out = resolve(root, 'build', 'extension');
const required = ['manifest.json', 'background.js', 'content.js', 'popup.html', 'options.html', 'offscreen.html'];
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
try {
  manifest = JSON.parse(readFileSync(resolve(out, 'manifest.json'), 'utf8'));
} catch (error) {
  failures.push('manifest.json is not valid JSON: ' + error.message);
}
if (manifest) {
  if (manifest.manifest_version !== 3) failures.push('manifest_version is not 3');
  if (manifest.version !== '1.4.0') failures.push('manifest version is not 1.4.0');
  if (!Array.isArray(manifest.permissions) || manifest.permissions.length !== 10)
    failures.push('permissions must contain exactly 10 entries');
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
  if (!manifest.options_ui || manifest.options_ui.page !== 'options.html')
    failures.push('options_ui must point at options.html');
}

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

const snapshot = resolve(root, '..', 'tests', 'conformance', 'schema-snapshot.v1.3.0.json');
const snapshotHash = createHash('sha256').update(readFileSync(snapshot)).digest('hex');
const generated = readFileSync(resolve(root, 'src', 'shared', 'contracts', 'tools.generated.ts'), 'utf8');
if (!generated.includes('schema-snapshot.v1.3.0.json')) failures.push('generated contracts do not name the snapshot');

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
process.stdout.write(JSON.stringify({ ok: true, files: files.length, sizes, assets, snapshotHash }, null, 2) + '\n');
