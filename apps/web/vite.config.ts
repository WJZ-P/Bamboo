import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// base 用相对路径：打包结果放在任何子路径下都能打开，比如 GitHub Pages 的 /Bamboo/。
// A relative base lets the build be served from any sub-path, such as /Bamboo/ on GitHub Pages.
export default defineConfig({
  base: "./",
  plugins: [react()],
  server: {
    port: 5273,
  },
});
