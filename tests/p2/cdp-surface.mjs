import { mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { McpClient, waitForToolCount } from './mcp-client.mjs';

const MCP = 'http://127.0.0.1:29473/mcp';
const FIXTURE = 'http://127.0.0.1:8835/page';
const EVID = 'C:/Users/Administrator/AppData/Local/Temp/corvus-v2-p2-qa/evidence/cdp-p2/cdp-surface.json';
const PUB = ['browser_status','tab_list','tab_open','tab_close','page_navigate','page_read','page_snapshot','element_click','element_type','page_screenshot'];
const checks = [];
function check(name, ok, detail) {
  checks.push({ name: name, ok: !!ok, detail: detail === undefined ? null : detail });
  if (!ok) console.log('FAIL ' + name + ' :: ' + JSON.stringify(detail === undefined ? null : detail).slice(0, 1200));
}
function keys(value) { return value && typeof value === 'object' ? Object.keys(value).sort() : null; }
async function sleep(ms) { await new Promise(function (resolve) { setTimeout(resolve, ms); }); }

let mcp;
try {
  mcp = new McpClient(MCP);
  const init = await mcp.initialize();
  const tools = await mcp.tools();
  check('initialize instructions exact', init && init.instructions === 'Browser tools appear when a browser session is connected. Use browser_status to see available groups.', init && init.instructions);
  check('ten public tools exact order', JSON.stringify(tools.map(function (t) { return t.name; })) === JSON.stringify(PUB), tools.map(function (t) { return t.name; }));
  const toolText = JSON.stringify(tools);
  check('tools/list lexical clean', !/\b(provider|extension|plugin|cdp|worker|websocket|channel)\b/i.test(toolText) && !/ws:\/\//i.test(toolText), toolText.slice(0, 300));

  const status = await mcp.call('browser_status', {});
  check('worker browser_status ok', !status.isError && status.value.count === 1, status.value);
  const browser = status.value.browsers && status.value.browsers[0];
  check('worker browser id is cd000001', !!browser && browser.browser_id === 'b-cd000001', browser);
  check('worker reports all groups', !status.isError && status.value.capabilities.every(function (c) { return c.available === true; }), status.value.capabilities);
  check('worker version is plain browser version', !!browser && browser.version === '157.0.8103.78', browser && browser.version);
  check('worker family Chrome', !!browser && browser.family === 'Chrome', browser && browser.family);
  const browserId = browser && browser.browser_id;

  const opened = await mcp.call('tab_open', { url: FIXTURE, active: true });
  check('worker tab_open works', !opened.isError && !!opened.value.tab_id && opened.value.url === FIXTURE, opened.value);
  const tabId = opened.value && opened.value.tab_id;
  await sleep(500);

  const nav = await mcp.call('page_navigate', { url: 'http://127.0.0.1:8835/second', tab_id: tabId, wait_until: 'load', timeout_ms: 10000 });
  check('worker page_navigate honors tab_id', !nav.isError && nav.value.tab_id === tabId && nav.value.url === 'http://127.0.0.1:8835/second', nav.value);
  const back = await mcp.call('page_navigate', { url: FIXTURE, tab_id: tabId, wait_until: 'load', timeout_ms: 10000 });
  check('worker page_navigate returns to fixture', !back.isError && back.value.tab_id === tabId && back.value.url === FIXTURE, back.value);

  const read = await mcp.call('page_read', { tab_id: tabId, max_chars: 20000 });
  check('worker page_read succeeds', !read.isError && /P2 Fixture|Hello P2 fixture/.test(read.value.content || ''), read.value);
  check('worker page_read has required keys', !read.isError && JSON.stringify(keys(read.value)) === JSON.stringify(['browser_id','content','mode','tab_id','truncated','url']), keys(read.value));

  const snap = await mcp.call('page_snapshot', { tab_id: tabId, max_nodes: 100 });
  check('worker page_snapshot succeeds', !snap.isError && Array.isArray(snap.value.nodes) && snap.value.nodes.length > 0, snap.value);
  check('worker page_snapshot required keys', !snap.isError && JSON.stringify(keys(snap.value)) === JSON.stringify(['browser_id','nodes','tab_id','truncated','url']), keys(snap.value));
  check('worker snapshot refs opaque', !snap.isError && snap.value.nodes.every(function (n) { return /^r-[0-9a-f]{8}$/.test(n.ref); }), (snap.value.nodes || []).slice(0, 4));
  check('worker snapshot no provider refs', !snap.isError && !JSON.stringify(snap.value).includes('private-node'), JSON.stringify(snap.value).slice(0, 200));

  const button = (snap.value.nodes || []).find(function (n) { return /button/i.test(n.role || '') && /click me/i.test(n.name || ''); });
  if (button) {
    const click = await mcp.call('element_click', { ref: button.ref, tab_id: tabId });
    check('worker element_click works', !click.isError && click.value.clicked === true && click.value.ref === button.ref, click.value);
    await sleep(200);
    const after = await mcp.call('page_read', { tab_id: tabId });
    check('worker click mutates fixture', !after.isError && /clicked/i.test(after.value.content || ''), after.value && after.value.content);
  } else check('worker click target found', false, (snap.value.nodes || []).slice(0, 20));

  const textbox = (snap.value.nodes || []).find(function (n) { return /textbox/i.test(n.role || '') && /name/i.test(n.name || ''); });
  if (textbox) {
    const type = await mcp.call('element_type', { ref: textbox.ref, tab_id: tabId, text: 'cdp-typed', clear: true });
    check('worker element_type works', !type.isError && type.value.typed_chars === 9 && type.value.ref === textbox.ref, type.value);
  } else check('worker type target found', false, (snap.value.nodes || []).slice(0, 20));

  const png = await mcp.call('page_screenshot', { tab_id: tabId, format: 'png', inline: false });
  let pngSize = -1, pngSha = '';
  if (!png.isError && png.value && png.value.path) { try { const bytes = readFileSync(png.value.path); pngSize = statSync(png.value.path).size; pngSha = createHash('sha256').update(bytes).digest('hex'); } catch {} }
  check('worker screenshot png works', !png.isError && png.image === null && !('data' in (png.value || {})) && pngSize > 0 && png.value.bytes === pngSize && png.value.sha256 === pngSha && png.value.mime_type === 'image/png', png.value);
  check('worker screenshot summary shape', !png.isError && png.value.summary === (png.value.width + 'x' + png.value.height + ' png ' + png.value.bytes + ' bytes'), png.value && png.value.summary);
  check('worker screenshot path shape', !png.isError && /shot-[0-9a-f]{32}\.png$/.test(png.value.path || ''), png.value && png.value.path);

  const jpeg = await mcp.call('page_screenshot', { tab_id: tabId, format: 'jpeg', quality: 80, inline: false });
  let jpegSize = -1, jpegSha = '';
  if (!jpeg.isError && jpeg.value && jpeg.value.path) { try { const bytes = readFileSync(jpeg.value.path); jpegSize = statSync(jpeg.value.path).size; jpegSha = createHash('sha256').update(bytes).digest('hex'); } catch {} }
  check('worker screenshot jpeg works', !jpeg.isError && jpegSize > 0 && jpeg.value.bytes === jpegSize && jpeg.value.sha256 === jpegSha && jpeg.value.mime_type === 'image/jpeg' && /\.jpg$/.test(jpeg.value.path || ''), jpeg.value);

  const inline = await mcp.call('page_screenshot', { tab_id: tabId, format: 'png', inline: true });
  check('worker screenshot inline image block', !inline.isError && inline.image !== null && !!inline.image.data && inline.value.inline === true && inline.value.image && inline.value.image.mime_type === 'image/png' && !('data' in inline.value.image), { imageKeys: inline.image && Object.keys(inline.image), valueKeys: keys(inline.value) });

  const full = await mcp.call('page_screenshot', { tab_id: tabId, full_page: true });
  check('worker full_page unavailable', full.isError && full.value.error.code === 'CAPABILITY_UNAVAILABLE', full.value);

  const closed = await mcp.call('tab_close', { tab_id: tabId });
  check('worker tab_close works', !closed.isError && closed.value.closed === true && closed.value.tab_id === tabId, closed.value);

  const finalStatus = await mcp.call('browser_status', { browser_id: browserId });
  check('worker status after actions', !finalStatus.isError && finalStatus.value.count === 1 && finalStatus.value.browsers[0].connected === true, finalStatus.value);
} catch (error) {
  check('cdp surface harness completed', false, String(error && error.stack || error));
}
const failed = checks.filter(function (c) { return !c.ok; });
const report = { generated_at: new Date().toISOString(), mcp: MCP, fixture: FIXTURE, checks_total: checks.length, passed: checks.length - failed.length, failed: failed.length, failed_names: failed.map(function (c) { return c.name; }), checks: checks };
try { mkdirSync(EVID.slice(0, EVID.lastIndexOf('/')), { recursive: true }); writeFileSync(EVID, JSON.stringify(report, null, 2)); } catch (error) { console.log('EVIDENCE WRITE ERROR ' + String(error)); }
console.log('RESULT ' + report.passed + ' passed, ' + report.failed + ' failed, ' + report.checks_total + ' checks');
console.log('EVIDENCE ' + EVID);
process.exit(report.failed === 0 ? 0 : 1);
