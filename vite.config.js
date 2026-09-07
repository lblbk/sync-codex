import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { handleApi } from "./server/api.js";

/** 把后端 API 挂到 Vite dev server 上（configureServer 是插件钩子，不是 server 配置项） */
function apiPlugin() {
  return {
    name: "sync-codex-api",
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        if (req.url?.startsWith("/api")) {
          handleApi(req, res);
        } else {
          next();
        }
      });
    },
  };
}

export default defineConfig({
  plugins: [svelte(), apiPlugin()],
  server: {
    host: "127.0.0.1",
    port: 8000,
    strictPort: true,
  },
});