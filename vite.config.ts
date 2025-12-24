import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import path from "path";

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    // Windows에서 Vite가 ::1(IPv6)로만 바인딩되면 Tauri WebView가 127.0.0.1로 붙으면서
    // dev server를 못 찾아 하얀 화면이 나올 수 있음. IPv4로 고정.
    host: "127.0.0.1",
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
