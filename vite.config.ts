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
    hmr: {
      overlay: false,
      clientLogLevel: "error",
    },
  },
  logLevel: "error",
  resolve: {
    alias: {
      $lib: path.resolve("./src/lib"),
    },
  },
  optimizeDeps: {
    include: ["@tauri-apps/api", "ethers", "qrcode", "html5-qrcode"],
    exclude: [
      "@tauri-apps/plugin-fs",
      "@tauri-apps/plugin-process",
      "@tauri-apps/plugin-shell",
    ],
  },
  build: {
    target: "esnext",
    minify: "esbuild",
    sourcemap: false,
    cssCodeSplit: true,
    chunkSizeWarningLimit: 1000,
    rollupOptions: {
      external: [
        "@tauri-apps/api/tauri",
        "@tauri-apps/plugin-fs",
        "@tauri-apps/plugin-process",
      ],
      output: {
        manualChunks: {
          "vendor-svelte": ["svelte", "svelte-i18n", "svelte-sonner"],
          "vendor-tauri": [
            "@tauri-apps/api",
            "@tauri-apps/plugin-dialog",
            "@tauri-apps/plugin-store",
          ],
          "vendor-ui": ["lucide-svelte", "@mateothegreat/svelte5-router"],
          "vendor-crypto": ["ethers"],
        },
      },
    },
  },
});
