/// <reference types="vitest" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { "@": path.resolve(import.meta.dirname, "./src") },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // Vite watching files being written by `cargo build` causes EBUSY on Windows.
    watch: { ignored: ["**/src-tauri/**"] },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    // `scripts/`: the build-time helpers are tested by the same runner, so
    // `npm test` covers the CHANGELOG extractor too.
    include: ["src/**/*.test.{ts,tsx}", "scripts/**/*.test.mjs"],
  },
});
