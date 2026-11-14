// Static fixture server for the M4 performance pack.
// Usage: node tests/perf/serve-fixture.mjs
// PORT=0 chooses a free port; the server prints "READY <base-url>" on stdout.
import { createServer } from "node:http";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const host = process.env.HOST || "127.0.0.1";
const requested = Number(process.env.PORT || 0);
const root = fileURLToPath(new URL("../fixtures/", import.meta.url));
const perfPage = root + "perf-10k.html";
const loadPage = "<!doctype html><html><head><title>Corvus perf load</title></head>" +
  "<body><button id=\"perf-click\" type=\"button\">Perf click</button>" +
  "<input id=\"perf-type\" type=\"text\" aria-label=\"Perf input\"></body></html>";

const server = createServer((req, res) => {
  const path = new URL(req.url || "/", "http://" + host).pathname;
  let body = null;
  let type = "text/html; charset=utf-8";
  if (path === "/" || path === "/perf-10k" || path === "/perf-10k.html") {
    body = readFileSync(perfPage);
  } else if (path === "/load") {
    body = Buffer.from(loadPage);
  }
  if (body === null) {
    res.writeHead(404, { "content-type": "text/plain; charset=utf-8" });
    res.end("not found");
    return;
  }
  res.writeHead(200, {
    "content-type": type,
    "content-length": body.length,
    "cache-control": "no-store",
  });
  res.end(body);
});

server.listen(requested, host, () => {
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : requested;
  console.log("READY http://" + host + ":" + port);
});

function stop() {
  server.close(() => process.exit(0));
  setTimeout(() => process.exit(0), 1000).unref();
}
process.on("SIGTERM", stop);
process.on("SIGINT", stop);
