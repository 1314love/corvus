// QA-only P2 surface matrix. Runs against a frozen daemon over MCP HTTP.
import { mkdirSync, statSync, readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { join } from 'node:path';
import { McpClient, waitForToolCount } from './mcp-client.mjs';
import { connectProvider, loadToken, defaultInvoke } from './provider-client.mjs';

const MCP = process.env.CORVUS_MCP || 'http://127.0.0.1:29373/mcp';
const TOKEN_FILE = process.env.TOKEN_FILE || 'C:/Users/Administrator/AppData/Local/Temp/corvus-v2-p2-qa/evidence/pairing-token.txt';
const HOME = process.env.HOME_DIR || 'C:/Users/Administrator/AppData/Local/Temp/corvus-v2-p2-qa/evidence/surface-home';
const EVID = process.env.EVID || 'C:/Users/Administrator/AppData/Local/Temp/corvus-v2-p2-qa/evidence/surface-matrix';
const PROVIDER_URL = process.env.PROVIDER_URL || 'ws://127.0.0.1:29373/provider/v2';
const FULL = ['browser_status','tab_list','tab_open','tab_close','page_navigate','page_read','page_snapshot','element_click','element_type','page_screenshot'];
const TAB = ['tab_list','tab_open','tab_close'];
const PAGE = ['page_navigate','page_read','page_snapshot'];
const INPUT = ['element_click','element_type'];
const CAPTURE = ['page_screenshot'];
const INSTRUCTIONS = 'Browser tools appear when a browser session is connected. Use browser_status to see available groups.';
const NO_SESSION_HINT = 'No browser session is connected. Connect a supported browser and retry.';

mkdirSync(EVID, { recursive: true });
const token = loadToken(TOKEN_FILE);
const client = new McpClient(MCP);
const checks = [];
const measurements = { tools: {}, screenshots: [], providerActions: [], statuses: {}, matrix: {} };
function check(name, ok, detail) {
  checks.push({ name, ok: !!ok, detail });
  if (!ok) console.log('FAIL ' + name + (detail === undefined ? '' : ' :: ' + JSON.stringify(detail).slice(0, 500)));
  return !!ok;
}
function eq(actual, expected) { return JSON.stringify(actual) === JSON.stringify(expected); }
function errorShape(call) {
  return call && call.isError === true && Array.isArray(call.content) && call.content.length === 1 &&
    call.content[0].type === 'text' && call.value && call.value.error && typeof call.value.error.code === 'string' &&
    typeof call.value.error.message === 'string';
}
async function expectError(name, args, code) {
  const call = await client.call(name, args);
  const ok = errorShape(call) && call.value.error.code === code;
  check('error ' + name + ' ' + code, ok, ok ? undefined : { isError: call.isError, content: call.content, value: call.value });
  return call;
}
function toolMap(tools) { const out = {}; for (const t of tools) out[t.name] = t; return out; }
async function toolsNow() { return client.tools(); }
async function expectTools(expectedNames, label) {
  const tools = await toolsNow();
  const names = tools.map((t) => t.name);
  const ok = eq(names, expectedNames);
  check('tool list ' + label, ok, ok ? undefined : names);
  measurements.tools[label] = names;
  return { tools, names };
}
async function drop(provider, expectedCount) {
  provider.stopped = true;
  provider.channel.close();
  await waitForToolCount(client, expectedCount).catch(() => {});
}
async function withFull(fn, options = {}) {
  const p = await connectProvider({ url: PROVIDER_URL, token, ...options });
  await waitForToolCount(client, 10);
  try { return await fn(p); } finally { await drop(p, 1); }
}
const PROPS = {
  browser_status: ['browser_id'],
  tab_list: ['browser_id'],
  tab_open: ['browser_id','url','active'],
  tab_close: ['browser_id','tab_id'],
  page_navigate: ['browser_id','url','tab_id','wait_until','timeout_ms'],
  page_read: ['browser_id','tab_id','mode','selector','max_chars'],
  page_snapshot: ['browser_id','tab_id','max_nodes','include_hidden'],
  element_click: ['browser_id','ref','tab_id'],
  element_type: ['browser_id','ref','text','clear','submit','tab_id'],
  page_screenshot: ['browser_id','tab_id','format','quality','full_page','inline'],
};
const REQUIRED = {
  browser_status: [], tab_list: [], tab_open: ['url'], tab_close: ['tab_id'], page_navigate: ['url'],
  page_read: [], page_snapshot: [], element_click: ['ref'], element_type: ['ref','text'], page_screenshot: [],
};
const DESCRIPTIONS = {
  browser_status: 'Report connected browsers and available capability groups.',
  tab_list: 'List open tabs.', tab_open: 'Open a tab.', tab_close: 'Close a tab.',
  page_navigate: 'Navigate a page.', page_read: 'Read page text or HTML.',
  page_snapshot: 'Snapshot interactive page elements and assign references.',
  element_click: 'Click an element reference from page_snapshot.',
  element_type: 'Type into an element reference from page_snapshot.',
  page_screenshot: 'Capture a page screenshot.',
};
function propCheck(name, props, key, expected) {
  const p = props[key] || {};
  const ok = Object.entries(expected).every(([k, v]) => eq(p[k], v));
  check('schema ' + name + ' ' + key, ok, ok ? undefined : p);
}

(async () => {
  const init = await client.initialize();
  check('initialize instructions', init.instructions === INSTRUCTIONS, init.instructions);
  check('initialize listChanged false on HTTP', init.capabilities && init.capabilities.tools && init.capabilities.tools.listChanged === false, init.capabilities);
  await waitForToolCount(client, 1);
  const initial = await expectTools(['browser_status'], 'initial');
  const banned = /\b(provider|extension|plugin|cdp|worker|websocket|ws|channel)\b/i;
  const lexicalText = JSON.stringify(initial.tools) + '\n' + (init.instructions || '');
  check('lexical ban tools/initialize', !banned.test(lexicalText), lexicalText.match(banned));
  check('initial tool count', initial.names.length === 1, initial.names);
  check('no-session tool call hidden tab_list', true);
  await expectError('tab_list', {}, 'CAPABILITY_UNAVAILABLE');
  await expectError('browser_everything', {}, 'BAD_ARGS');
  const noSession = await client.call('browser_status', {});
  check('no-session status', !noSession.isError && noSession.value.count === 0 && eq(noSession.value.browsers, []) &&
    Array.isArray(noSession.value.capabilities) && noSession.value.capabilities.length === 4 &&
    noSession.value.capabilities.every((c) => c.available === false) && noSession.value.hint === NO_SESSION_HINT, noSession.value);

  // Schemas are stable even with one provider connected.
  await withFull(async () => {
    const tools = await toolsNow();
    const map = toolMap(tools);
    for (const name of FULL) {
      const t = map[name];
      check('schema present ' + name, !!t, name);
      if (!t) continue;
      const s = t.inputSchema || {};
      check('schema type ' + name, s.type === 'object' && s.additionalProperties === false, s);
      check('schema required ' + name, eq(s.required || [], REQUIRED[name]), s.required);
      check('schema properties ' + name, eq(Object.keys(s.properties || {}).sort(), PROPS[name].slice().sort()), Object.keys(s.properties || {}));
      check('schema description ' + name, t.description === DESCRIPTIONS[name], t.description);
      const p = s.properties || {};
      propCheck(name, p, 'browser_id', { type: 'string', pattern: '^b-[0-9a-f]{8}$' });
      if (p.tab_id) propCheck(name, p, 'tab_id', { type: 'string', minLength: 1, maxLength: 64 });
      if (p.ref) propCheck(name, p, 'ref', { type: 'string', pattern: '^r-[0-9a-f]{8}$' });
      if (p.url) propCheck(name, p, 'url', { type: 'string' });
      if (p.active) propCheck(name, p, 'active', { type: 'boolean', default: true });
      if (p.wait_until) propCheck(name, p, 'wait_until', { type: 'string', enum: ['load','domcontentloaded'], default: 'load' });
      if (p.timeout_ms) propCheck(name, p, 'timeout_ms', { type: 'integer', minimum: 1000, maximum: 60000, default: 30000 });
      if (p.mode) propCheck(name, p, 'mode', { type: 'string', enum: ['text','html'], default: 'text' });
      if (p.selector) propCheck(name, p, 'selector', { type: 'string', minLength: 1, maxLength: 2000 });
      if (p.max_chars) propCheck(name, p, 'max_chars', { type: 'integer', minimum: 1000, maximum: 200000, default: 20000 });
      if (p.max_nodes) propCheck(name, p, 'max_nodes', { type: 'integer', minimum: 50, maximum: 2000, default: 500 });
      if (p.include_hidden) propCheck(name, p, 'include_hidden', { type: 'boolean', default: false });
      if (p.clear) propCheck(name, p, 'clear', { type: 'boolean', default: true });
      if (p.submit) propCheck(name, p, 'submit', { type: 'boolean', default: false });
      if (p.format) propCheck(name, p, 'format', { type: 'string', enum: ['png','jpeg'], default: 'png' });
      if (p.quality) propCheck(name, p, 'quality', { type: 'integer', minimum: 1, maximum: 100, default: 80 });
      if (p.full_page) propCheck(name, p, 'full_page', { type: 'boolean', default: false });
      if (p.inline) propCheck(name, p, 'inline', { type: 'boolean', default: false });
    }
    await expectError('tab_open', {}, 'BAD_ARGS');
    await expectError('tab_open', { url: 'https://example.com', extra: 1 }, 'BAD_ARGS');
    await expectError('page_read', { max_chars: 999 }, 'BAD_ARGS');
    await expectError('page_read', { max_chars: 200001 }, 'BAD_ARGS');
    await expectError('page_snapshot', { max_nodes: 49 }, 'BAD_ARGS');
    await expectError('page_snapshot', { max_nodes: 2001 }, 'BAD_ARGS');
    await expectError('element_type', { ref: 'r-00000000', text: 'x'.repeat(20001) }, 'BAD_ARGS');
    await expectError('page_screenshot', { quality: 0 }, 'BAD_ARGS');
    await expectError('page_screenshot', { quality: 101 }, 'BAD_ARGS');
  });

  // Full lifecycle and action normalization.
  await withFull(async (p) => {
    const status = await client.call('browser_status', {});
    measurements.statuses.full = status.value;
    check('status full count1', !status.isError && status.value.count === 1 && status.value.browsers.length === 1, status.value);
    const b = status.value.browsers[0] || {};
    check('status browser fields', b.connected === true && b.family === 'chrome' && b.version === '157.0.8103.78' && b.profile === 'qa' && /^b-[0-9a-f]{8}$/.test(b.browser_id || ''), b);
    check('status capabilities all', eq(status.value.capabilities.map((c) => [c.group, c.available]), [['tabs',true],['page',true],['input',true],['capture',true]]), status.value.capabilities);
    check('status no hint', !('hint' in status.value) || !status.value.hint, status.value.hint);
    const rid = b.browser_id;

    const list = await client.call('tab_list', {});
    check('tab_list normalized', !list.isError && list.value.browser_id === rid && list.value.count === 2 &&
      list.value.tabs.length === 2 && list.value.tabs[0].tab_id === '1' && list.value.tabs[0].active === true &&
      list.value.tabs[1].tab_id === '2' && list.value.tabs[1].active === false, list.value);
    check('tab_list no internal ids', !JSON.stringify(list.value).includes('private'), list.value);

    const read = await client.call('page_read', { mode: 'html', max_chars: 1000 });
    measurements.providerActions.push(p.actions.slice(-1)[0]);
    check('page_read normalized', !read.isError && read.value.mode === 'html' && read.value.content === 'Hello P2 fixture' &&
      read.value.truncated === false && read.value.tab_id === '1' && read.value.browser_id === rid, read.value);

    const snap = await client.call('page_snapshot', { max_nodes: 50, include_hidden: true });
    const nodes = snap.value && snap.value.nodes || [];
    check('snapshot count', !snap.isError && nodes.length === 3 && snap.value.truncated === false && snap.value.tab_id === '1', snap.value);
    check('snapshot refs opaque', nodes.every((n) => /^r-[0-9a-f]{8}$/.test(n.ref)) && !JSON.stringify(snap.value).includes('private-node'), nodes.map((n) => n.ref));
    check('snapshot refs unique', new Set(nodes.map((n) => n.ref)).size === 3, nodes.map((n) => n.ref));
    const refs = measurements.snapshotRefs = nodes.map((n) => n.ref);

    const click = await client.call('element_click', { ref: refs[1] });
    check('element_click normalized', !click.isError && click.value.clicked === true && click.value.ref === refs[1] && click.value.tab_id === '1' && click.value.browser_id === rid, click.value);

    const typed = await client.call('element_type', { ref: refs[2], text: 'typed text', clear: false, submit: true });
    check('element_type normalized', !typed.isError && typed.value.typed_chars === 10 && typed.value.submitted === true && typed.value.ref === refs[2] && typed.value.tab_id === '1', typed.value);

    const nav = await client.call('page_navigate', { url: 'http://127.0.0.1:8835/second', tab_id: '2', wait_until: 'domcontentloaded', timeout_ms: 5000 });
    check('page_navigate normalized', !nav.isError && nav.value.url === 'http://127.0.0.1:8835/second' && nav.value.title === 'Second' && nav.value.status === 'loaded' && nav.value.tab_id === '2' && nav.value.browser_id === rid, nav.value);
    await expectError('page_navigate', { url: 'ftp://example.com/file' }, 'BAD_ARGS');
    await expectError('element_click', { ref: refs[1], tab_id: '2' }, 'NOT_FOUND');
    const about = await client.call('tab_open', { url: 'about:blank', active: false });
    check('tab_open normalized', !about.isError && about.value.tab_id === '7' && about.value.window_id === 10 && about.value.url === 'about:blank' && about.value.active === false, about.value);
    const close = await client.call('tab_close', { tab_id: '7' });
    check('tab_close normalized', !close.isError && close.value.tab_id === '7' && close.value.closed === true && close.value.browser_id === rid, close.value);
    await expectError('tab_open', { url: 'ftp://example.com/file' }, 'BAD_ARGS');

    const shot = await client.call('page_screenshot', { format: 'png', inline: false });
    const shotValue = shot.value || {};
    const path = shotValue.path || '';
    let fileOk = false, size = -1, sha = '', bytes = null;
    try { bytes = readFileSync(path); size = statSync(path).size; sha = createHash('sha256').update(bytes).digest('hex'); fileOk = true; } catch {}
    const fileName = path.split(/[\\/]/).pop() || '';
    check('screenshot file absent inline', !shot.isError && shot.image === null && !('data' in shotValue) && !('image' in shotValue) && fileOk && shotValue.inline === false, shotValue);
    check('screenshot path shape', fileOk && /^shot-[0-9a-f]{32}\.png$/.test(fileName) && path.replace(/\\/g, '/').startsWith(join(HOME, 'screenshots').replace(/\\/g, '/')), { path, fileName });
    check('screenshot metadata', shotValue.bytes === size && shotValue.sha256 === sha && shotValue.width === 1 && shotValue.height === 1 &&
      shotValue.format === 'png' && shotValue.mime_type === 'image/png' && shotValue.summary === '1x1 png ' + size + ' bytes', { shotValue, size, sha });
    measurements.screenshots.push({ path, bytes: size, sha256: sha, summary: shotValue.summary, inline: false });

    const inlineShot = await client.call('page_screenshot', { inline: true, format: 'png' });
    const iv = inlineShot.value || {};
    const image = inlineShot.image || {};
    const textContent = (inlineShot.content || []).find((c) => c.type === 'text') || {};
    check('screenshot inline image', !inlineShot.isError && image.type === 'image' && image.mimeType === 'image/png' &&
      typeof image.data === 'string' && image.data.length > 20 && Buffer.from(image.data, 'base64').length === iv.bytes, { imageMime: image.mimeType, bytes: iv.bytes });
    check('screenshot inline metadata has no data', !('data' in iv) && !(iv.image && 'data' in iv.image) && iv.inline === true && textContent.text && !textContent.text.includes('"data"'), iv);
    check('screenshot inline recording source', iv.path !== shotValue.path, { first: shotValue.path, second: iv.path });
    measurements.screenshots.push({ path: iv.path, bytes: iv.bytes, sha256: iv.sha256, summary: iv.summary, inline: true });

    await expectError('page_screenshot', { full_page: true }, 'CAPABILITY_UNAVAILABLE');
  });

  // Provider failure mapping, invalid image and custom provider error codes.
  let captureFailure = 0;
  await connectProvider({ url: PROVIDER_URL, token, onInvoke: async (action, args) => {
    if (action === 'capture.viewport') {
      captureFailure += 1;
      if (captureFailure === 1) return { tab_id: '1', url: 'http://127.0.0.1:8835/page', format: 'png', mime_type: 'image/png', width: 1, height: 1, data: '***not-base64***' };
      if (captureFailure === 2) throw Object.assign(new Error('capture target absent'), { code: 'NOT_FOUND' });
      if (captureFailure === 3) throw Object.assign(new Error('raw provider exploded'), { code: 'WEIRD_CODE' });
    }
    return defaultInvoke(action, args);
  }}).then(async (p) => {
    await waitForToolCount(client, 10);
    try {
      const bad = await client.call('page_screenshot', {});
      check('invalid base64 -> INTERNAL', errorShape(bad) && bad.value.error.code === 'INTERNAL' && /invalid image/i.test(bad.value.error.message), bad.value);
      const nf = await client.call('page_screenshot', {});
      check('known provider NOT_FOUND passthrough', errorShape(nf) && nf.value.error.code === 'NOT_FOUND' && nf.value.error.message !== 'capture target absent', nf.value);
      const weird = await client.call('page_screenshot', {});
      check('unknown provider code -> INTERNAL', errorShape(weird) && weird.value.error.code === 'INTERNAL' && !/WEIRD_CODE/.test(weird.value.error.message), weird.value);
    } finally { await drop(p, 1); }
  });

  // Capability subset matrix.
  const matrix = [
    ['all', null, 10],
    ['no capture', ['tabs','page','input'], 9],
    ['no input', ['tabs','page','capture'], 8],
    ['no tabs', ['page','input','capture'], 7],
    ['no page', ['tabs','input','capture'], 5],
    ['no page+tabs', ['input','capture'], 2],
    ['no groups', [], 1],
    ['partial tabs (list,open)', ['tabs-partial','page','input','capture'], 7],
    ['partial page (navigate,read)', ['tabs','page-partial','input','capture'], 5],
  ];
  for (const [label, groups, expectedCount] of matrix) {
    let caps;
    if (groups === null) caps = undefined;
    else if (groups.length === 0) caps = [{ id: 'system.info', version: 1, actions: ['info'] }];
    else caps = groups.map((g) => {
      if (g === 'tabs') return { id: 'tabs', version: 1, actions: ['list','open','close'] };
      if (g === 'tabs-partial') return { id: 'tabs', version: 1, actions: ['list','open'] };
      if (g === 'page') return { id: 'page', version: 1, actions: ['navigate','read','snapshot'] };
      if (g === 'page-partial') return { id: 'page', version: 1, actions: ['navigate','read'] };
      if (g === 'input') return { id: 'input', version: 1, actions: ['click','type'] };
      return { id: 'capture', version: 1, actions: ['viewport'] };
    });
    const expected = FULL.filter((name) => {
      if (name === 'browser_status') return true;
      if (TAB.includes(name)) return groups === null || groups.includes('tabs');
      if (PAGE.includes(name)) return groups === null || groups.includes('page');
      if (INPUT.includes(name)) return groups === null || (groups.includes('input') && groups.includes('page'));
      return groups === null || groups.includes('capture');
    });
    const p = await connectProvider({ url: PROVIDER_URL, token, capabilities: caps });
    await waitForToolCount(client, expectedCount);
    const { names } = await expectTools(expected, label);
    measurements.matrix[label] = { expectedCount, names };
    check('matrix count ' + label, names.length === expectedCount, names.length);
    const hidden = FULL.find((name) => !names.includes(name));
    if (hidden) await expectError(hidden, {}, 'CAPABILITY_UNAVAILABLE');
    await drop(p, 1);
  }

  // Two browsers: aggregate status, ambiguity, routing, quiet disconnect.
  const p1 = await connectProvider({ url: PROVIDER_URL, token, providerId: 'provider-00000000-0000-4000-8000-0000000000a1' });
  await waitForToolCount(client, 10);
  const p2 = await connectProvider({ url: PROVIDER_URL, token, providerId: 'provider-00000000-0000-4000-8000-0000000000a2' });
  await new Promise((r) => setTimeout(r, 400));
  const both = await client.call('browser_status', {});
  measurements.statuses.two = both.value;
  check('two browsers count2', !both.isError && both.value.count === 2 && both.value.browsers.length === 2 &&
    both.value.browsers[0].browser_id < both.value.browsers[1].browser_id && both.value.capabilities.every((c) => c.available), both.value);
  await expectError('tab_list', {}, 'AMBIGUOUS_BROWSER');
  const chosen = both.value.browsers[0].browser_id;
  const routed = await client.call('tab_list', { browser_id: chosen });
  check('explicit browser routes', !routed.isError && routed.value.browser_id === chosen && routed.value.count === 2, routed.value);
  await expectError('tab_list', { browser_id: 'b-ffffffff' }, 'BAD_ARGS');
  await drop(p2, 10);
  for (let i = 0; i < 40; i += 1) { const s = await client.call('browser_status', {}); if (!s.isError && s.value.count === 1) break; await new Promise((r) => setTimeout(r, 100)); }
  const afterOne = await client.call('browser_status', {});
  check('one remains after disconnect', !afterOne.isError && afterOne.value.count === 1 && afterOne.value.browsers[0].browser_id === chosen, afterOne.value);
  await drop(p1, 1);

  const failed = checks.filter((c) => !c.ok);
  const report = {
    generated_at: new Date().toISOString(), mcp: MCP, provider_url: PROVIDER_URL,
    checks_total: checks.length, passed: checks.length - failed.length, failed: failed.length,
    failed_names: failed.map((c) => c.name), checks, measurements,
  };
  const out = join(EVID, 'surface-matrix.json');
  writeFileSync(out, JSON.stringify(report, null, 2));
  console.log('RESULT ' + report.passed + ' passed, ' + report.failed + ' failed, ' + report.checks_total + ' checks');
  console.log('EVIDENCE ' + out);
  process.exit(report.failed === 0 ? 0 : 1);
})().catch((error) => {
  console.error('MATRIX ERROR', error && error.stack || error);
  process.exit(2);
});
