import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Dev 阶段把 API 请求代理到本机 anole-server，生产阶段由 server 同源托管
// 静态资源（ANOLE_WEB_DIR），无需 CORS。
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1421,
    strictPort: true,
    proxy: {
      "/v1": "http://127.0.0.1:8787",
      "/health": "http://127.0.0.1:8787",
      "/openapi.json": "http://127.0.0.1:8787",
    },
  },
  envPrefix: ["VITE_"],
  build: {
    minify: "oxc",
    sourcemap: false,
  },
});
