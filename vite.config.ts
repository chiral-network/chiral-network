import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import path from "path";

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    watch: {
      ignored: ["**/src-tauri/**", "**/target/**", "**/relay/target/**"],
    },
  },
  resolve: {
    alias: {
      $lib: path.resolve("./src/lib"),
    },
  },
  optimizeDeps: {
    include: ["@tauri-apps/api"],
  },
  build: {
    target: "esnext",
    minify: "esbuild",
    rollupOptions: {
      external: ["@tauri-apps/api/tauri", "@tauri-apps/plugin-fs", "@tauri-apps/plugin-process"],
    },
  },
});
