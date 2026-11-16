// QA-only fixture server for the v2 P2 surface tests.
import { createServer } from 'node:http';

const PORT = Number(process.env.PORT || 8835);
const PAGE = String.raw`<!doctype html>
<html><head><meta charset="utf-8"><title>P2 Fixture</title>
<style>
  body { font-family: sans-serif; margin: 16px; }
  #hidden-display { display: none; }
  #hidden-visibility { visibility: hidden; }
  #zero { width: 0; height: 0; overflow: hidden; }
</style></head>
<body>
  <h1>P2 Fixture</h1>
  <p id="intro">Hello P2 fixture</p>
  <a id="link-one" href="/second">Second page</a>
  <a id="link-two" href="https://example.com/two">Example</a>
  <button id="btn" onclick="document.getElementById('intro').textContent='clicked'; window.__clicked=(window.__clicked||0)+1;">Click me</button>
  <form id="form" action="/submitted" method="post">
    <label for="name-input">Name</label>
    <input id="name-input" name="name" value="Alice">
    <label for="password-input">Password</label>
    <input id="password-input" type="password" value="not-returned">
    <button id="submit" type="submit">Submit</button>
  </form>
  <input id="type-target" aria-label="Type target" value="before">
  <div id="hidden-display">Hidden display</div>
  <div id="hidden-visibility">Hidden visibility</div>
  <div id="zero">Zero area</div>
  <div aria-hidden="true" id="aria-hidden">Aria hidden</div>
  <div id="shadow-host"></div>
  <script>
    const host = document.getElementById('shadow-host');
    const root = host.attachShadow({ mode: 'open' });
    const shadowButton = document.createElement('button');
    shadowButton.textContent = 'Shadow button';
    root.appendChild(shadowButton);
  </script>
</body></html>`;
const SECOND = String.raw`<!doctype html><html><head><title>Second</title></head><body><h1>Second page</h1><p>Second body</p></body></html>`;
const server = createServer((req, res) => {
  if (req.url === '/page' || req.url === '/') {
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
    res.end(PAGE);
    return;
  }
  if (req.url === '/second') {
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
    res.end(SECOND);
    return;
  }
  if (req.url === '/submitted') {
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
    res.end('<!doctype html><html><head><title>Submitted</title></head><body><h1>Submitted</h1><p>Form received</p></body></html>');
    return;
  }
  res.writeHead(404, { 'content-type': 'text/plain' });
  res.end('not found');
});
server.listen(PORT, '127.0.0.1', () => console.log('FIXTURE_READY ' + PORT));
process.on('SIGTERM', () => server.close(() => process.exit(0)));
process.on('SIGINT', () => server.close(() => process.exit(0)));
