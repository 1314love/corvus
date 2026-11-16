import { createServer, type Server } from 'node:http';

export interface FixtureServer {
  origin: string;
  crossOrigin: string;
  close: () => Promise<void>;
}

function topPage(crossOrigin: string): string {
  return [
    '<!doctype html>',
    '<html><body style="margin:0">',
    '<button id="top-button" style="position:absolute;left:10px;top:10px;width:120px;height:32px">Top Action</button>',
    '<div id="shadow-host" style="position:absolute;left:10px;top:60px;width:200px;height:40px"></div>',
    "<iframe id=\"same-frame\" srcdoc=\"<!doctype html><html><body style='margin:0'><button id='frame-button' style='position:absolute;left:10px;top:10px;width:140px;height:32px'>Frame Action</button><script>document.getElementById('frame-button').addEventListener('click',function(){this.dataset.clicked='yes';});</script></body></html>\" style=\"position:absolute;left:100px;top:120px;width:400px;height:300px;border:7px solid #333\"></iframe>",
    '<iframe id="cross-frame" src="' +
      crossOrigin +
      '/frame.html" style="position:absolute;left:10px;top:450px;width:300px;height:200px;border:1px solid #333"></iframe>',
    '<button id="focus-button" style="position:absolute;left:700px;top:30px;width:120px;height:32px">Focus Me</button>',
    '<button id="drag-source" type="button" style="position:absolute;left:650px;top:300px;width:60px;height:40px">Drag source</button>',
    '<button id="drag-target" type="button" style="position:absolute;left:850px;top:300px;width:60px;height:40px">Drag target</button>',
    '<div id="links" style="position:absolute;left:10px;top:760px">',
    '  <a id="link-docs" href="/docs" title="Documentation">Docs</a>',
    '  <a id="link-mail" href="mailto:help@example.com">Mail</a>',
    '  <a id="link-hidden" href="/hidden" style="display:none">Hidden</a>',
    '</div>',
    '<form id="profile-form" action="/submit" method="post" style="position:absolute;left:10px;top:820px">',
    '  <input id="full-name" name="full_name" aria-label="Full name" value="Ada">',
    '  <input id="password" name="password" type="password" value="secret">',
    '  <input id="terms" name="terms" type="checkbox" checked>',
    '  <select id="plan" name="plan" aria-label="Plan"><option value="free">Free</option><option value="pro" selected>Pro</option></select>',
    '  <button type="submit">Save</button>',
    '</form>',
    '<div style="height:2200px;width:1px"></div>',
    '<script>',
    "  var shadowRoot = document.getElementById('shadow-host').attachShadow({ mode: 'open' });",
    `  shadowRoot.innerHTML = '<button id="shadow-button" style="position:absolute;left:20px;top:10px;width:140px;height:32px">Shadow Action</button>';`,
    "  var shadowButton = shadowRoot.getElementById('shadow-button');",
    "  shadowButton.addEventListener('click', function () { this.dataset.clicked = 'yes'; });",
    "  document.getElementById('top-button').addEventListener('click', function () { this.dataset.clicked = 'yes'; });",
    '</script>',
    '</body></html>',
  ].join('\n');
}

export async function startFixtureServer(port: number): Promise<FixtureServer> {
  const origin = 'http://127.0.0.1:' + String(port);
  const crossOrigin = 'http://localhost:' + String(port);
  const html = topPage(crossOrigin);

  const server: Server = createServer((request, response) => {
    response.setHeader('cache-control', 'no-store');
    if (request.url === '/frames.html') {
      response.setHeader('content-type', 'text/html; charset=utf-8');
      response.end(html);
      return;
    }
    if (request.url === '/frame.html') {
      response.setHeader('content-type', 'text/html; charset=utf-8');
      response.end('<!doctype html><html><body><p>Cross-origin frame</p></body></html>');
      return;
    }
    response.statusCode = 404;
    response.end('not found');
  });

  await new Promise<void>((resolve, reject) => {
    server.once('error', reject);
    server.listen(port, '0.0.0.0', () => {
      server.off('error', reject);
      resolve();
    });
  });

  return {
    origin,
    crossOrigin,
    close: async () => {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) => {
        server.close((error) => (error ? reject(error) : resolve()));
      });
    },
  };
}
