import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Tauri 要求固定端口；清屏会把 Rust 的编译错误刷掉，所以关掉。
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
});
