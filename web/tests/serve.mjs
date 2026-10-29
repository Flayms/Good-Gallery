// A static server for the built page (`target/web`) on localhost, which browsers treat as secure like HTTPS.
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../target/web/", import.meta.url));
const port = Number(process.env.PORT || 8765);
const types = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".css": "text/css",
  ".wasm": "application/wasm",
  ".svg": "image/svg+xml",
};

createServer(async (request, response) => {
  const path = normalize(decodeURIComponent(new URL(request.url, "http://localhost").pathname));
  const file = join(root, path.endsWith("/") ? path + "index.html" : path);
  if (!file.startsWith(root)) {
    response.writeHead(403).end();
    return;
  }
  try {
    const body = await readFile(file);
    response.writeHead(200, { "Content-Type": types[extname(file)] || "application/octet-stream" }).end(body);
  } catch {
    response.writeHead(404).end();
  }
}).listen(port, "127.0.0.1", () => console.log(`serving ${root} on http://localhost:${port}/`));
