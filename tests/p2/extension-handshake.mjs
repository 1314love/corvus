// QA-only P2 real built-extension handshake. Frozen build, no artifact patching.
import { spawn } from 'node:child_process';
import { mkdirSync, statSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { join } from 'node:path';
import { McpClient, waitForToolCount } from './mcp-client.mjs';
import { loadToken } from './provider-client.mjs';

const CHROME = process.env.CHROME || 'C:/Users/Administrator/AppData/Local/Temp/corvus-chrome/chrome-win64/chrome.exe';
const EXT = process.env.EXT || 'D:/DSH/browser_bridege/corvus/extension/build/extension';
const MCP = process.env.MCP_URL || 'http://127.0.0.1:29373/mcp';
const BASE = process.env.BASE_URL || 'ws://127.0.0.1:29373';
const DEBUG_PORT = Number(process.env.DEBUG_PORT || 9356);
const PROFILE = process.env.EXT_PROFILE || 'C:/Users/Administrator/AppData/Local/Temp/corvus-v2-p2-qa/evidence/ext-p2-profile';
const EVID = process.env.EXT_EVID || 'C:/Users/Administrator/AppData/Local/Temp/corvus-v2-p2-qa/evidence/ext-p2';
const EXPECT_BROWSERS = Number(process.env.EXPECT_BROWSERS || 1);
const TOKEN_FILE = 'C:/Users/Administrator/AppData/Local/Temp/corvus-v2-p2-qa/evidence/pairing-token.txt';
mkdirSync(EVID, { recursive: true });
try { rmSync(PROFILE, { recursive: true, force: true }); } catch {}
const token = loadToken(TOKEN_FILE);
const checks = [];
const measurements = { console: [], manifest: null, providerState: null, status: null };
function check(name, ok, detail) { checks.push({ name, ok: !!ok, detail }); if (!ok) console.log('FAIL ' + name + ' :: ' + JSON.stringify(detail === undefined ? null : detail).slice(0, 500)); }
async function expectError(name, args, code) { const call = await mcp.call(name, args); const error = call.value && call.value.error; const ok = call.isError === true && !!error && error.code === code; check('error ' + name + ' ' + code, ok, ok ? undefined : { isError: call.isError, value: call.value }); return call; }
function sleep(ms) { return new Promise(function (resolve) { setTimeout(resolve, ms); }); }
async function fetchJson(path) { const response = await fetch('http://127.0.0.1:' + DEBUG_PORT + path); return response.json(); }
async function pollTargets(predicate, timeoutMs) {
  const started = Date.now(); let targets = [];
  while (Date.now() - started < timeoutMs) {
    try { targets = await fetchJson('/json/list'); } catch {}
    const found = targets.find(predicate);
    if (found) return found;
    await sleep(200);
  }
  throw new Error('target not found; last=' + JSON.stringify(targets.map(function (t) { return [t.type, t.url]; })));
}
class Cdp {
  constructor(url) { this.url = url; this.id = 0; this.pending = new Map(); this.events = []; this.waiters = []; }
  async open() {
    this.ws = new WebSocket(this.url);
    await new Promise(function (resolve, reject) { this.ws.addEventListener('open', resolve, { once: true }); this.ws.addEventListener('error', reject, { once: true }); }.bind(this));
    const self = this;
    this.ws.addEventListener('message', function (event) {
      const message = JSON.parse(event.data);
      if (message.id && self.pending.has(message.id)) { const entry = self.pending.get(message.id); self.pending.delete(message.id); if (message.error) entry.reject(new Error(JSON.stringify(message.error))); else entry.resolve(message.result); }
      else { self.events.push(message); self.waiters = self.waiters.filter(function (waiter) { if (waiter.predicate(message)) { waiter.resolve(message); return false; } return true; }); }
    });
    return this;
  }
  send(method, params) {
    const id = ++this.id;
    const self = this;
    return new Promise(function (resolve, reject) { self.pending.set(id, { resolve: resolve, reject: reject }); self.ws.send(JSON.stringify({ id: id, method: method, params: params || {} })); });
  }
  close() { try { this.ws.close(); } catch {} }
}
async function evaluate(cdp, expression) {
  const result = await cdp.send('Runtime.evaluate', { expression: expression, awaitPromise: true, returnByValue: true });
  if (result.exceptionDetails) throw new Error('evaluate exception ' + JSON.stringify(result.exceptionDetails));
  return result.result && result.result.value;
}
const chrome = spawn(CHROME, ['--headless=new','--remote-debugging-port=' + String(DEBUG_PORT),'--user-data-dir=' + PROFILE,'--no-first-run','--no-default-browser-check','--disable-gpu','--no-sandbox','--disable-dev-shm-usage','--disable-extensions-except=' + EXT,'--load-extension=' + EXT], { stdio: ['ignore','pipe','pipe'] });
let chromeStderr = '';
chrome.stderr.setEncoding('utf8'); chrome.stderr.on('data', function (chunk) { chromeStderr += chunk; });
let worker = null, offscreen = null, mcp = null;
try {
  for (let i = 0; i < 60; i += 1) { try { await fetchJson('/json/version'); break; } catch { await sleep(200); } }
  const offTarget0 = await pollTargets(function (t) { return t.url.includes('/offscreen.html'); }, 20000);
  const extensionId = new URL(offTarget0.url).host;
  const swTarget = await pollTargets(function (t) { return t.type === 'service_worker' && new URL(t.url).host === extensionId; }, 15000);
  check('extension service worker present', !!extensionId, swTarget.url);
  worker = await new Cdp(swTarget.webSocketDebuggerUrl).open();
  await worker.send('Runtime.enable');
  const manifest = await evaluate(worker, 'chrome.runtime.getManifest()');
  measurements.manifest = manifest;
  const permissions = (manifest.permissions || []).slice().sort();
  check('manifest version 1.7.0', manifest.version === '1.7.0', manifest.version);
  check('manifest exactly five permissions', JSON.stringify(permissions) === JSON.stringify(['activeTab','offscreen','scripting','storage','tabs']), permissions);
  check('manifest no debugger', permissions.indexOf('debugger') === -1, permissions);
  check('manifest host all urls', JSON.stringify(manifest.host_permissions || []) === JSON.stringify(['<all_urls>']), manifest.host_permissions);
  check('manifest content all_frames false', !manifest.content_scripts || manifest.content_scripts.every(function (s) { return s.all_frames !== true; }), manifest.content_scripts);

  const setupExpression = "(async () => { await chrome.storage.local.set({ 'corvus.daemonUrl': " + JSON.stringify(BASE) + ", 'provider.pairingToken': " + JSON.stringify(token.text) + " }); try { await chrome.offscreen.closeDocument(); } catch {} return 'stored'; })()";
  const setup = await evaluate(worker, setupExpression);
  check('storage keys set', setup === 'stored', setup);
  for (let attempt = 0; attempt < 10; attempt += 1) {
    const created = await evaluate(worker, "(async () => { try { await chrome.offscreen.createDocument({ url: chrome.runtime.getURL('offscreen.html'), reasons: ['WORKERS'], justification: 'QA provider connection.' }); return 'created'; } catch (error) { return String(error && error.message || error); } })()");
    if (created === 'created') break;
    await sleep(200);
  }
  const offTarget = await pollTargets(function (t) { return t.url.includes('/offscreen.html') && t.id !== offTarget0.id; }, 15000);
  offscreen = await new Cdp(offTarget.webSocketDebuggerUrl).open();
  await offscreen.send('Runtime.enable');
  await offscreen.send('Log.enable');
  const connectExpression = "chrome.runtime.sendMessage({ target: 'offscreen', type: 'provider.connect', base_url: " + JSON.stringify(BASE) + ", token: " + JSON.stringify(token.text) + " })";
  const connectReply = await evaluate(worker, connectExpression);
  check('provider connect message accepted', !!connectReply && connectReply.ok !== false, connectReply);

  mcp = new McpClient(MCP);
  await mcp.initialize();
  let status = null;
  for (let i = 0; i < 60; i += 1) {
    const call = await mcp.call('browser_status', {});
    if (!call.isError && call.value && call.value.count >= EXPECT_BROWSERS) { status = call.value; break; }
    await sleep(250);
  }
  measurements.status = status;
  const browsers = status && Array.isArray(status.browsers) ? status.browsers : [];
  const workerBrowser = browsers.find(function (item) { return item.browser_id === 'b-cd000001'; });
  const browser = (EXPECT_BROWSERS > 1 ? browsers.find(function (item) { return item.browser_id !== 'b-cd000001'; }) : browsers[0]) || {};
  const browserId = browser.browser_id;
  check('real extension connected to daemon', !!status && status.count === EXPECT_BROWSERS && !!browser.connected && (EXPECT_BROWSERS === 1 || !!workerBrowser), status);
  check('real extension family chrome', String(browser.family).toLowerCase() === 'chrome', browser);
  check('real extension profile displayed', typeof browser.profile === 'string' && browser.profile.length > 0, browser.profile);
  check('real extension version is Chrome 157.x', typeof browser.version === 'string' && /^157\./.test(browser.version), browser.version);
  check('real extension all groups', !!status && status.capabilities.every(function (c) { return c.available === true; }), status && status.capabilities);
  const names = await waitForToolCount(mcp, 10);
  check('real extension exposes ten tools', names.length === 10, names);
  const stateReply = await evaluate(worker, "chrome.runtime.sendMessage({ target: 'offscreen', type: 'provider.status_request' })");
  measurements.providerState = stateReply;
  check('provider socket reports connected state', !!stateReply && stateReply.ok === true && !!stateReply.state && stateReply.state.status === 'connected', stateReply);
  check('provider state has session id', !!stateReply && !!stateReply.state && typeof stateReply.state.session_id === 'string' && stateReply.state.session_id.startsWith('s-'), stateReply && stateReply.state && stateReply.state.session_id);

  const tabs = await mcp.call('tab_list', { browser_id: browserId });
  check('real tab_list returns at least one tab', !tabs.isError && tabs.value.count >= 1 && Array.isArray(tabs.value.tabs), tabs.value);
  const opened = await mcp.call('tab_open', { browser_id: browserId, url: 'http://127.0.0.1:8835/page', active: true });
  check('real tab_open works', !opened.isError && !!opened.value.tab_id && opened.value.url === 'http://127.0.0.1:8835/page', opened.value);
  const tabId = opened.value && opened.value.tab_id;
  await sleep(400);
  const read = await mcp.call('page_read', { browser_id: browserId, tab_id: tabId });
  measurements.read = read.value;
  check('real page_read content works', !read.isError && /P2 Fixture|Hello P2 fixture/.test(read.value.content || ''), read.value);
  check('real page_read includes tab_id', !read.isError && read.value.tab_id === tabId, read.value);
  const snap = await mcp.call('page_snapshot', { browser_id: browserId, tab_id: tabId, max_nodes: 100 });
  measurements.snapshot = snap.value;
  check('real page_snapshot succeeds', !snap.isError, snap.value);
  check('real page_snapshot nodes', !snap.isError && Array.isArray(snap.value.nodes) && snap.value.nodes.length > 0, snap.value);
  check('real page_snapshot includes tab_id', !snap.isError && snap.value.tab_id === tabId, snap.value);
  const snapshotRefs = !snap.isError && Array.isArray(snap.value.nodes) ? snap.value.nodes.map(function (node) { return node.ref; }) : [];
  check('real page_snapshot refs opaque', snapshotRefs.length > 0 && snapshotRefs.every(function (ref) { return /^r-[0-9a-f]{8}$/.test(ref); }), snapshotRefs);
  check('real page_snapshot refs unique', new Set(snapshotRefs).size === snapshotRefs.length, snapshotRefs);
  const button = (snap.value.nodes || []).find(function (node) { return node.role === 'button' && /click me/i.test(node.name || ''); });
  if (button) {
    const clicked = await mcp.call('element_click', { browser_id: browserId, ref: button.ref, tab_id: tabId });
    check('real element_click works', !clicked.isError && clicked.value.clicked === true && clicked.value.ref === button.ref, clicked.value);
    await sleep(300);
    const after = await mcp.call('page_read', { browser_id: browserId, tab_id: tabId });
    check('real click mutates fixture', !after.isError && /clicked/i.test(after.value.content || ''), after.value && after.value.content);
  } else check('real click target found', false, snap.value && snap.value.nodes);
  const textbox = (snap.value.nodes || []).find(function (node) { return node.role === 'textbox' && /name/i.test(node.name || ''); });
  if (textbox) {
    const typed = await mcp.call('element_type', { browser_id: browserId, ref: textbox.ref, tab_id: tabId, text: 'qa-typed', clear: true });
    check('real element_type works', !typed.isError && typed.value.typed_chars === 8 && typed.value.ref === textbox.ref, typed.value);
  } else check('real type target found', false, snap.value && snap.value.nodes);

  const shot = await mcp.call('page_screenshot', { browser_id: browserId, tab_id: tabId, format: 'png', inline: false });
  measurements.png = shot.value;
  let size = -1, sha = '', pngSignature = false;
  if (!shot.isError && shot.value && shot.value.path) { try { const bytes = readFileSync(shot.value.path); size = statSync(shot.value.path).size; sha = createHash('sha256').update(bytes).digest('hex'); pngSignature = bytes.length > 8 && bytes[0] === 0x89 && bytes[1] === 0x50 && bytes[2] === 0x4e && bytes[3] === 0x47 && bytes[4] === 0x0d && bytes[5] === 0x0a && bytes[6] === 0x1a && bytes[7] === 0x0a; } catch {} }
  check('real screenshot png output', !shot.isError && shot.image === null && !('data' in (shot.value || {})) && size > 0 && shot.value.bytes === size && shot.value.sha256 === sha && shot.value.mime_type === 'image/png' && pngSignature, shot.value);
  check('real screenshot name unique', !shot.isError && /^shot-[0-9a-f]{32}\.png$/.test((shot.value.path || '').split(/[\\/]/).pop() || ''), shot.value && shot.value.path);
  const shotJpeg = await mcp.call('page_screenshot', { browser_id: browserId, tab_id: tabId, format: 'jpeg', quality: 80, inline: false });
  measurements.jpeg = shotJpeg.value;
  let jpegSize = -1, jpegSha = '';
  if (!shotJpeg.isError && shotJpeg.value && shotJpeg.value.path) { try { const bytes = readFileSync(shotJpeg.value.path); jpegSize = statSync(shotJpeg.value.path).size; jpegSha = createHash('sha256').update(bytes).digest('hex'); } catch {} }
  check('real screenshot jpeg output', !shotJpeg.isError && shotJpeg.image === null && jpegSize > 0 && shotJpeg.value.bytes === jpegSize && shotJpeg.value.sha256 === jpegSha && shotJpeg.value.mime_type === 'image/jpeg' && /\.jpg$/.test(shotJpeg.value.path || ''), shotJpeg.value);
  const fullPage = await mcp.call('page_screenshot', { browser_id: browserId, tab_id: tabId, full_page: true });
  check('real full_page unavailable', fullPage.isError && fullPage.value.error.code === 'CAPABILITY_UNAVAILABLE', fullPage.value);
  const navigated = await mcp.call('page_navigate', { browser_id: browserId, tab_id: tabId, url: 'http://127.0.0.1:8835/second', wait_until: 'domcontentloaded', timeout_ms: 5000 });
  check('real page_navigate works', !navigated.isError && navigated.value.tab_id === tabId && navigated.value.url === 'http://127.0.0.1:8835/second' && navigated.value.status === 'loaded', navigated.value);
  const extraTab = await mcp.call('tab_open', { browser_id: browserId, url: 'about:blank', active: false });
  const closedTab = extraTab.isError ? extraTab : await mcp.call('tab_close', { browser_id: browserId, tab_id: extraTab.value.tab_id });
  check('real tab_close works', !extraTab.isError && !closedTab.isError && closedTab.value.closed === true && closedTab.value.tab_id === extraTab.value.tab_id, { open: extraTab.value, close: closedTab.value });

  if (EXPECT_BROWSERS > 1) {
    check('combined status has worker browser', !!workerBrowser && workerBrowser.connected === true, workerBrowser);
    check('combined browser ids distinct', !!browserId && !!workerBrowser && browserId !== workerBrowser.browser_id, { extension: browserId, worker: workerBrowser && workerBrowser.browser_id });
    check('combined aggregate groups', !!status && status.capabilities.length === 4 && status.capabilities.every(function (item) { return item.available === true; }), status && status.capabilities);
    const workerTabs = await mcp.call('tab_list', { browser_id: 'b-cd000001' });
    check('tab_list routes to worker browser', !workerTabs.isError && workerTabs.value.browser_id === 'b-cd000001' && workerTabs.value.count >= 1, workerTabs.value);
    const workerShot = await mcp.call('page_screenshot', { browser_id: 'b-cd000001', format: 'png', inline: false });
    let workerFile = false;
    if (!workerShot.isError && workerShot.value && workerShot.value.path) { try { workerFile = statSync(workerShot.value.path).size === workerShot.value.bytes; } catch {} }
    check('capture routes to worker browser', !workerShot.isError && workerShot.value.browser_id === 'b-cd000001' && workerFile && workerShot.value.mime_type === 'image/png', workerShot.value);
    const extTabs = await mcp.call('tab_list', { browser_id: browserId });
    check('tab_list routes to extension browser', !extTabs.isError && extTabs.value.browser_id === browserId, extTabs.value);
    await expectError('tab_list', {}, 'AMBIGUOUS_BROWSER');
  }

  const logEntries = offscreen.events.filter(function (event) { return event.method === 'Log.entryAdded'; }).map(function (event) { return event.params.entry; });
  const consoleEntries = offscreen.events.filter(function (event) { return event.method === 'Runtime.consoleAPICalled'; });
  const allText = JSON.stringify(consoleEntries) + JSON.stringify(logEntries);
  measurements.console = { log_entries: logEntries.length, console_entries: consoleEntries.length, sample: logEntries.slice(0, 3) };
  check('extension console has no token', allText.indexOf(token.text) === -1, measurements.console);
  check('daemon sees no forbidden wording in AI status', !/provider|extension|cdp|worker|websocket|ws:\/\//i.test(JSON.stringify(status)), status);
} catch (error) {
  check('extension harness completed', false, String(error && error.stack || error));
}
if (offscreen) offscreen.close();
if (worker) worker.close();
try { chrome.kill(); } catch {}
await sleep(500);
const failed = checks.filter(function (c) { return !c.ok; });
const report = { generated_at: new Date().toISOString(), chrome: CHROME, extension: EXT, debug_port: DEBUG_PORT, checks_total: checks.length, passed: checks.length - failed.length, failed: failed.length, failed_names: failed.map(function (c) { return c.name; }), checks: checks, measurements: measurements, chrome_stderr_excerpt: chromeStderr.slice(0, 1500), token_leaked_in_stderr: chromeStderr.indexOf(token.text) !== -1 };
const out = join(EVID, 'extension-handshake.json');
writeFileSync(out, JSON.stringify(report, null, 2));
console.log('RESULT ' + report.passed + ' passed, ' + report.failed + ' failed, ' + report.checks_total + ' checks');
console.log('EVIDENCE ' + out);
process.exit(report.failed === 0 ? 0 : 1);
