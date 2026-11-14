// Static server for the M1 and M2 acceptance pages. English only by project rule.
// Usage: node tests/acceptance/fixtures/serve.mjs
// Optional: PORT=8808 HOST=127.0.0.1 node tests/acceptance/fixtures/serve.mjs
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const host = process.env.HOST || "127.0.0.1";
const port = Number(process.env.PORT || 8808);
const fixture = (name) => fileURLToPath(new URL("./" + name, import.meta.url));

const routes = new Map([
  ["/", fixture("live-page.html")],
  ["/index.html", fixture("live-page.html")],
  ["/m2", fixture("m2-page.html")],
  ["/m2.html", fixture("m2-page.html")],
  ["/m2-b", fixture("m2-b.html")],
  ["/m2-b.html", fixture("m2-b.html")],
  ["/m2-c", fixture("m2-c.html")],
  ["/m2-c.html", fixture("m2-c.html")],
]);

const server = createServer(async (req, res) => {
  const path = new URL(req.url || "/", "http://" + host).pathname;
  const file = routes.get(path);
  if (req.method === "GET" && file) {
    try {
      const body = await readFile(file);
      res.writeHead(200, {
        "content-type": "text/html; charset=utf-8",
        "cache-control": "no-store",
      });
      res.end(body);
    } catch (error) {
      res.writeHead(500, { "content-type": "text/plain; charset=utf-8" });
      res.end("fixture read failed: " + error.message);
    }
    return;
  }
  res.writeHead(404, { "content-type": "text/plain; charset=utf-8" });
  res.end("not found");
});

server.listen(port, host, () => {
  console.log("serving http://" + host + ":" + port + "/");
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => server.close(() => process.exit(0)));
}
