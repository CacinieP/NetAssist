import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // 3. tell vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
  // 4. isolate the heaviest vendor (echarts ~1MB) into its own cacheable chunk
  //    so app-code changes don't force users to re-download it. Other vendors
  //    are left to Rollup's default chunking to avoid circular chunks.
  build: {
    // echarts itself is ~1MB unminified; its isolated vendor chunk legitimately
    // exceeds Vite's default 500KB limit. Raise the floor rather than split a
    // single library. App code chunks stay well under this.
    chunkSizeWarningLimit: 1100,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes("node_modules")) {
            if (id.includes("echarts")) return "echarts-vendor";
            if (id.includes("i18next")) return "i18n-vendor";
          }
        },
      },
    },
  },
}));
