// QA-only P2 stdio dynamic-surface harness with two providers.
import { spawn } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { loadToken, connectProvider } from './provider-client.mjs';

const BIN = process.env.CORVUSD || 'C:/Users/Administrator/AppData/Local/Temp/corvus-v2-p2-final-target/debug/corvusd.exe';
const TOKEN_FILE = 'C:/Users/Administrator/AppData/Local/Temp/corvus-v2-p2-qa/evidence/pairing-token.txt';
const EVID = 'C:/Users/Administrator/AppData/Local/Temp/corvus-v2-p2-qa/evidence/stdio-multi';
const HOME = join(EVID, 'home');
const DB = join(EVID, 'stdio.db');
const WS_PORT = 29376;
const HTTP_PORT = 29377;
const PROVIDER_URL = 'ws://127.0.0.1:' + HTTP_PORT + '/provider/v2';
mkdirSync(EVID, { recursive: true });
const token = loadToken(TOKEN_FILE);
const checks = [];
function check(name, ok, detail) { checks.push({ name: name, ok: !!ok, detail: detail === undefined ? null : detail }); if (!ok) console.log('FAIL ' + name + ' :: ' + JSON.stringify(detail === undefined ? null : detail).slice(0, 500)); }
const child = spawn(BIN, ['serve','--bind','127.0.0.1','--ws-port',String(WS_PORT),'--http-port',String(HTTP_PORT),'--home',HOME,'--db',DB,'--no-cdp','--channel','extension','--stdio','--log-file',join(EVID,'daemon.log')], { stdio: ['pipe','pipe','pipe'], env: { ...process.env, CORVUS_V2_SURFACE: '1', CORVUS_PAIRING_TOKEN: token.text } });
let stdout = '';
let stderr = '';
let nextId = 1;
let buffer = '';
const messages = [];
const waiters = [];
const nonJson = [];
child.stdout.setEncoding('utf8');
child.stdout.on('data', (chunk) => {
  stdout += chunk; buffer += chunk;
  for (;;) { const index = buffer.indexOf('\n'); if (index < 0) break; const line = buffer.slice(0, index).trim(); buffer = buffer.slice(index + 1); if (!line) continue; try { messages.push(JSON.parse(line)); } catch { nonJson.push(line); } }
  pump();
});
child.stderr.setEncoding('utf8');
child.stderr.on('data', (chunk) => { stderr += chunk; });
function pump() {
  for (let i = waiters.length - 1; i >= 0; i -= 1) {
    const waiter = waiters[i];
    const found = messages.find(waiter.predicate);
    if (found) { clearTimeout(waiter.timer); waiters.splice(i, 1); waiter.resolve(found); }
  }
}
function waitFor(predicate, timeoutMs = 5000, label = 'message') {
  const found = messages.find(predicate);
  if (found) return Promise.resolve(found);
  return new Promise((resolve, reject) => {
    const waiter = { predicate: predicate, resolve: resolve };
    waiter.timer = setTimeout(() => { const index = waiters.indexOf(waiter); if (index >= 0) waiters.splice(index, 1); reject(new Error('timed out waiting for ' + label)); }, timeoutMs);
    waiters.push(waiter);
  });
}
function send(object) { child.stdin.write(JSON.stringify(object) + '\n'); }
async function rpc(method, params) {
  const id = nextId++;
  send({ jsonrpc: '2.0', id: id, method: method, params: params });
  const reply = await waitFor((m) => m.id === id, 8000, method);
  if (reply.error) throw new Error(method + ' JSON-RPC error ' + JSON.stringify(reply.error));
  return reply.result;
}
const notificationCount = () => messages.filter((m) => m.method === 'notifications/tools/list_changed').length;
async function waitForNotification(count, timeoutMs = 6000) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    if (notificationCount() >= count) return true;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  return false;
}
async function toolNames() { const result = await rpc('tools/list', {}); return (result.tools || []).map((tool) => tool.name); }
async function connectRetry(options, attempts = 30) {
  let last;
  for (let i = 0; i < attempts; i += 1) {
    try { return await connectProvider({ url: PROVIDER_URL, token: token, ...options }); } catch (error) { last = error; await new Promise((resolve) => setTimeout(resolve, 200)); }
  }
  throw last;
}
function closeProvider(provider) { if (!provider) return; try { provider.stopped = true; provider.channel.close(); } catch {} }
let p1 = null;
let p2 = null;
let exitCode = null;
child.on('exit', (code) => { exitCode = code; });
try {
  const initialized = await rpc('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'p2-stdio-multi-qa', version: '1.0.0' } });
  check('stdio initialize listChanged true', initialized && initialized.capabilities && initialized.capabilities.tools && initialized.capabilities.tools.listChanged === true, initialized && initialized.capabilities);
  send({ jsonrpc: '2.0', method: 'notifications/initialized' });
  check('stdio initial one tool', JSON.stringify(await toolNames()) === JSON.stringify(['browser_status']), await toolNames());

  p1 = await connectRetry({ providerId: 'provider-00000000-0000-4000-8000-0000000000c1' });
  check('first provider notifies', await waitForNotification(1), notificationCount());
  const full = await toolNames();
  check('first provider full ten tools', full.length === 10, full);

  p2 = await connectRetry({ providerId: 'provider-00000000-0000-4000-8000-0000000000c2' });
  await new Promise((resolve) => setTimeout(resolve, 1200));
  check('second provider emits no change', notificationCount() === 1, notificationCount());
  check('second provider still ten tools', (await toolNames()).length === 10, await toolNames());
  const status = await rpc('tools/call', { name: 'browser_status', arguments: {} });
  const statusValue = JSON.parse(status.content[0].text);
  check('two providers status count two', statusValue.count === 2 && statusValue.browsers.length === 2, statusValue);

  closeProvider(p1); p1 = null;
  await new Promise((resolve) => setTimeout(resolve, 1200));
  check('one provider left emits no change', notificationCount() === 1, notificationCount());
  check('one provider left still ten tools', (await toolNames()).length === 10, await toolNames());

  closeProvider(p2); p2 = null;
  check('last provider notifies removal', await waitForNotification(2), notificationCount());
  check('last provider leaves one tool', JSON.stringify(await toolNames()) === JSON.stringify(['browser_status']), await toolNames());
  check('exactly two change notifications', notificationCount() === 2, notificationCount());

  child.stdin.end();
  await new Promise((resolve) => { if (exitCode !== null) return resolve(); const timer = setTimeout(resolve, 3000); child.on('exit', () => { clearTimeout(timer); resolve(); }); });
  check('stdio process exits', exitCode !== null, exitCode);
  check('stdio stdout all JSON', nonJson.length === 0, nonJson.slice(0, 3));
  check('stdio no token on stdout/stderr', !stdout.includes(token.text) && !stderr.includes(token.text), { out: stdout.length, err: stderr.length });
} catch (error) {
  check('stdio multi harness completed', false, String(error && error.stack || error));
}
closeProvider(p1); closeProvider(p2);
if (exitCode === null) { try { child.kill(); } catch {} }
const failed = checks.filter((c) => !c.ok);
const report = { generated_at: new Date().toISOString(), binary: BIN, ws_port: WS_PORT, http_port: HTTP_PORT, checks_total: checks.length, passed: checks.length - failed.length, failed: failed.length, failed_names: failed.map((c) => c.name), checks: checks, notifications: notificationCount(), exit_code: exitCode, stderr_excerpt: stderr.slice(0, 2000), stdout_bytes: stdout.length };
const out = join(EVID, 'stdio-listchange-multi.json');
writeFileSync(out, JSON.stringify(report, null, 2));
console.log('RESULT ' + report.passed + ' passed, ' + report.failed + ' failed, ' + report.checks_total + ' checks');
console.log('EVIDENCE ' + out);
process.exit(report.failed === 0 ? 0 : 1);
