import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import path from 'path';

const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [svelte()],
  
  resolve: {
    alias: {
      '$lib': path.resolve('./src/lib')
    }
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell vite to ignore watching `src-tauri`
      ignored: ['**/src-tauri/**'],
    },
  },

  build: {
    // Split the bundle into a few coarse chunks so the Tauri webview
    // doesn't have to parse 750 kB of JS in a single blocking step on
    // cold start. Goals (in order):
    //  - vendor: third-party libs that change rarely; cache hit on
    //    repeat loads.
    //  - tauri: the @tauri-apps/api surface, dynamically imported from
    //    every page.
    //  - icons: lucide-svelte tree, static-imported broadly enough that
    //    factoring it out shrinks the index chunk.
    rollupOptions: {
      output: {
        manualChunks(id: string) {
          if (!id.includes('node_modules')) return;
          if (id.includes('lucide-svelte')) return 'icons';
          if (id.includes('@tauri-apps')) return 'tauri';
          return 'vendor';
        },
      },
    },
  },
});
