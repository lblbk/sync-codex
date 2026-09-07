import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { handleApi, PORT } from "./api.js";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const DIST_DIR = path.join(__dirname, "..", "dist");

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".webp": "image/webp",
  ".gif": "image/gif",
  ".ico": "image/x-icon",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
  ".ttf": "font/ttf",
  ".map": "application/json; charset=utf-8",
  ".txt": "text/plain; charset=utf-8",
};

/** 把请求 URL 解析成 dist 目录内的安全相对路径，越权路径返回 null */
function resolveAssetPath(url) {
  if (!url) return null;
  const pathname = decodeURIComponent(url.split("?")[0].split("#")[0]);
  const relative = pathname === "/" ? "/index.html" : pathname;
  const filePath = path.normalize(path.join(DIST_DIR, relative));
  if (filePath !== DIST_DIR && !filePath.startsWith(DIST_DIR + path.sep)) {
    return null;
  }
  return filePath;
}

function serveStatic(req, res) {
  const filePath = resolveAssetPath(req.url);
  if (!filePath) {
    res.writeHead(403, { "Content-Type": "text/plain; charset=utf-8" });
    res.end("403 Forbidden");
    return;
  }

  const isHtmlNavigation = path.extname(filePath) === ".html";

  fs.readFile(filePath, (err, data) => {
    if (err) {
      if (isHtmlNavigation) {
        // SPA 回退：只对页面导航返回 index.html
        fs.readFile(path.join(DIST_DIR, "index.html"), (htmlErr, html) => {
          if (htmlErr) {
            res.writeHead(500, { "Content-Type": "text/plain; charset=utf-8" });
            res.end("500 Internal Server Error");
            return;
          }
          res.writeHead(200, {
            "Content-Type": MIME[".html"],
            "Cache-Control": "no-cache",
          });
          res.end(html);
        });
        return;
      }
      // 资源文件缺失必须 404，否则浏览器会把 HTML 当 CSS/JS 解析
      res.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
      res.end("404 Not Found");
      return;
    }

    const type = MIME[path.extname(filePath)] || "application/octet-stream";
    // 带内容哈希的产物资源可以长缓存，页面本身不缓存
    const cacheControl = isHtmlNavigation
      ? "no-cache"
      : "public, max-age=31536000, immutable";
    res.writeHead(200, {
      "Content-Type": type,
      "Cache-Control": cacheControl,
      "Content-Length": data.length,
    });
    res.end(req.method === "HEAD" ? undefined : data);
  });
}

const server = http.createServer((req, res) => {
  const url = req.url || "/";

  if (url.startsWith("/api")) {
    handleApi(req, res);
    return;
  }

  if (req.method === "GET" || req.method === "HEAD") {
    serveStatic(req, res);
    return;
  }

  res.writeHead(405, { "Content-Type": "text/plain; charset=utf-8", Allow: "GET, HEAD" });
  res.end("405 Method Not Allowed");
});

server.listen(PORT, () => {
  console.log("========================================");
  console.log("  Codex 远程模型同步工具");
  console.log("========================================");
  console.log("  访问地址: http://localhost:" + PORT);
  console.log("========================================");
});