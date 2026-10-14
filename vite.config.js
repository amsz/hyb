import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// 原包 dev server 端口为 1420（见 exe 内字符串 http://localhost:1420）
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2021",
    assetsDir: "assets",
  },
});
