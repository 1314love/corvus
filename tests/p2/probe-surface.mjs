import { connectProvider, loadToken } from './provider-client.mjs';
import { McpClient } from './mcp-client.mjs';
const token = loadToken(process.env.TOKEN_FILE || 'C:/Users/Administrator/AppData/Local/Temp/corvus-v2-p2-qa/evidence/pairing-token.txt');
const p = await connectProvider({ url: process.env.PROVIDER_URL || 'ws://127.0.0.1:29372/provider/v2', token, providerId: 'provider-00000000-0000-4000-8000-000000000001' });
console.log('CONNECTED', p.providerId, p.sessionId);
const c = new McpClient(process.env.MCP_URL || 'http://127.0.0.1:29373/mcp');
const init = await c.initialize();
console.log('INIT', JSON.stringify(init.instructions));
let names = [];
for (let i = 0; i < 40; i += 1) {
  names = (await c.tools()).map((t) => t.name);
  if (names.length === 10) break;
  await new Promise((r) => setTimeout(r, 150));
}
console.log('TOOLS', JSON.stringify(names));
const st = await c.call('browser_status', {});
console.log('STATUS', JSON.stringify(st.value));
const tabs = await c.call('tab_list', {});
console.log('TABS', JSON.stringify(tabs.value));
const snap = await c.call('page_snapshot', {});
console.log('SNAP', JSON.stringify(snap.value));
const click = await c.call('element_click', { ref: snap.value.nodes[1].ref });
console.log('CLICK', JSON.stringify(click.value));
p.stopped = true;
p.channel.close();
process.exit(0);
