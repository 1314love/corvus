const ws = new WebSocket(process.env.URL || 'ws://127.0.0.1:29372/provider/v2');
ws.addEventListener('open', () => console.log('OPEN'));
ws.addEventListener('message', (e) => { console.log('MESSAGE', typeof e.data, String(e.data).slice(0, 400)); ws.close(); });
ws.addEventListener('close', (e) => { console.log('CLOSE', e.code, e.reason); process.exit(0); });
ws.addEventListener('error', (e) => console.log('ERROR', e.message || 'error'));
setTimeout(() => { console.log('TIMEOUT'); ws.close(); }, 4000);
