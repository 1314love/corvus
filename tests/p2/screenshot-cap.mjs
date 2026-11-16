import { existsSync, readdirSync, writeFileSync } from 'node:fs';
import { McpClient, waitForToolCount } from './mcp-client.mjs';
import { connectProvider, loadToken } from './provider-client.mjs';

const BASE = process.env.P2_BASE || 'http://127.0.0.1:29373';
const MCP = BASE + '/mcp';
const PROVIDER = 'ws://127.0.0.1:29373/provider/v2';
const TOKEN = loadToken(process.env.TOKEN_FILE || 'C:/Users/Administrator/AppData/Local/Temp/corvus-v2-p2-qa/evidence/pairing-token.txt');
const HOME = process.env.HOME_DIR || 'C:/Users/Administrator/AppData/Local/Temp/corvus-v2-p2-qa/evidence/surface-home';
const SHOTS = HOME + '/screenshots';
const EVID = process.env.EVID || 'C:/Users/Administrator/AppData/Local/Temp/corvus-v2-p2-qa/evidence/p2-cap';
const checks = [];
function check(name, ok, detail) {
  checks.push({ name: name, ok: !!ok, detail: detail === undefined ? null : detail });
  if (!ok) console.log('FAIL ' + name + ' :: ' + JSON.stringify(detail === undefined ? null : detail).slice(0, 800));
}
function shotFiles() { try { return readdirSync(SHOTS).filter(function (name) { return /^shot-/.test(name); }).sort(); } catch { return []; } }
const before = shotFiles();
const big = Buffer.alloc(8 * 1024 * 1024 + 1, 0x41).toString('base64');
let provider = null;
try {
  provider = await connectProvider({
    url: PROVIDER,
    token: TOKEN,
    browser: { family: 'chrome', version: '157.0.8103.78', profile: 'cap' },
    limits: { max_invoke_bytes: 1048576, max_result_bytes: 25165824 },
    onInvoke: function (action, args) {
      if (action === 'capture.viewport') {
        return { tab_id: '1', url: 'http://127.0.0.1:8835/page', format: 'png', mime_type: 'image/png', width: 10, height: 10, data: big };
      }
      if (action === 'tabs.list') return { tabs: [{ tab_id: '1', window_id: 10, title: 'Home', url: 'http://127.0.0.1:8835/page', active: true }], count: 1 };
      if (action === 'page.navigate') return { tab_id: '1', url: 'http://127.0.0.1:8835/page', title: 'P2 Fixture', status: 'loaded' };
      if (action === 'page.read') return { tab_id: '1', url: 'http://127.0.0.1:8835/page', mode: 'text', content: 'fixture', truncated: false };
      if (action === 'page.snapshot') return { tab_id: '1', url: 'http://127.0.0.1:8835/page', nodes: [], truncated: false };
      if (action === 'tabs.open') return { tab_id: '9', window_id: 10, url: args.url, active: true };
      if (action === 'tabs.close') return { tab_id: args.tab_id, closed: true };
      if (action === 'input.click') return { tab_id: '1', ref: args.ref, clicked: true };
      if (action === 'input.type') return { tab_id: '1', ref: args.ref, typed_chars: String(args.text || '').length, submitted: false };
      throw Object.assign(new Error('unexpected action ' + action), { code: 'BAD_ARGS' });
    }
  });
  const mcp = new McpClient(MCP);
  await mcp.initialize();
  const names = await waitForToolCount(mcp, 10, 15000);
  check('ten tools after fake provider', names.length === 10, names);
  const shot = await mcp.call('page_screenshot', { tab_id: '1', format: 'png', quality: 80, inline: false });
  check('oversized screenshot rejected', shot.isError === true, shot.value);
  check('oversized screenshot error code', shot.isError && shot.value.error && shot.value.error.code === 'INTERNAL', shot.value);
  check('oversized screenshot exact message', shot.isError && shot.value.error && shot.value.error.message === 'Screenshot exceeds the 8 MiB limit.', shot.value);
  check('oversized screenshot no image block', shot.image === null, shot.image && Object.keys(shot.image));
  check('oversized screenshot no data field', !shot.value || !('data' in shot.value), shot.value && Object.keys(shot.value));
  const after = shotFiles();
  check('oversized screenshot retains no file', after.length === before.length && after.every(function (name, index) { return name === before[index]; }), { before: before, after: after });
  check('provider saw capture action', provider.actions.indexOf('capture.viewport') >= 0, provider.actions);
} catch (error) {
  check('cap harness completed', false, String(error && error.stack || error));
} finally {
  if (provider) { provider.stopped = true; try { provider.channel.close(); } catch {} }
}
const failed = checks.filter(function (c) { return !c.ok; });
const report = { generated_at: new Date().toISOString(), mcp: MCP, screenshots_dir: SHOTS, checks_total: checks.length, passed: checks.length - failed.length, failed: failed.length, failed_names: failed.map(function (c) { return c.name; }), checks: checks };
import { mkdirSync } from 'node:fs';
mkdirSync(EVID, { recursive: true });
writeFileSync(EVID + '/screenshot-cap.json', JSON.stringify(report, null, 2));
console.log('RESULT ' + report.passed + ' passed, ' + report.failed + ' failed, ' + report.checks_total + ' checks');
console.log('EVIDENCE ' + EVID + '/screenshot-cap.json');
process.exit(report.failed === 0 ? 0 : 1);
