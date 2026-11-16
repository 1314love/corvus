// QA-only MCP HTTP client for the frozen ten-tool surface.
export class McpClient {
  constructor(url) { this.url = url; this.session = null; this.id = 0; }
  async rpc(method, params) {
    const body = JSON.stringify({ jsonrpc: '2.0', id: ++this.id, method, params });
    const headers = { 'content-type': 'application/json', accept: 'application/json, text/event-stream' };
    if (this.session) headers['mcp-session-id'] = this.session;
    const response = await fetch(this.url, { method: 'POST', headers, body });
    const session = response.headers.get('mcp-session-id');
    if (session) this.session = session;
    const text = await response.text();
    let payload;
    try { payload = JSON.parse(text); }
    catch {
      const dataLine = text.split(/\r?\n/).find((line) => line.startsWith('data: '));
      if (!dataLine) throw new Error('non-JSON MCP response: ' + text.slice(0, 200));
      payload = JSON.parse(dataLine.slice(6));
    }
    if (payload.error) throw Object.assign(new Error('MCP error ' + payload.error.code + ': ' + payload.error.message), { rpc: payload.error });
    return payload.result;
  }
  async initialize() { return this.rpc('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'p2-qa', version: '1.0.0' } }); }
  async tools() { const result = await this.rpc('tools/list', {}); return result.tools || []; }
  async call(name, args = {}) {
    const result = await this.rpc('tools/call', { name, arguments: args });
    const content = result.content || [];
    const text = content.find((item) => item.type === 'text');
    let value = null;
    if (text) { try { value = JSON.parse(text.text); } catch { value = { raw: text.text }; } }
    return { result, content, value, isError: !!result.isError, image: content.find((item) => item.type === 'image') || null };
  }
}

export async function waitForToolCount(client, expected, timeoutMs = 15000) {
  const started = Date.now();
  let names = [];
  while (Date.now() - started < timeoutMs) {
    names = (await client.tools()).map((tool) => tool.name);
    if (names.length === expected) return names;
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error('tools never reached ' + expected + ' entries; last=' + names.join(','));
}
