// Static server for the M1 acceptance page. English only by project rule.
// Usage: node tests/acceptance/fixtures/serve.mjs
// Optional: PORT=8808 HOST=127.0.0.1 node tests/acceptance/fixtures/serve.mjs
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const host = process.env.HOST || "127.0.0.1";
const port = Number(process.env.PORT || 8808);
const fixture = fileURLToPath(new URL("./live-page.html", import.meta.url));

const server = createServer(async (req, res) => {
  if (req.method === "GET" && (req.url === "/" || req.url === "/index.html")) {
    try {
      const body = await readFile(fixture);
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
