/// <reference types="vitest/config" />
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  // Dev only: forward API + MCP calls to the Rust server so `pnpm dev` (Vite) and
  // the backend share an origin. The production build is served by the Rust binary.
  server: {
    proxy: {
      "/api": "http://127.0.0.1:8000",
      "/mcp": "http://127.0.0.1:8000",
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test-setup.ts"],
    coverage: {
      provider: "v8",
      reporter: ["text", "lcov"],
      include: ["src/**"],
      // Entry/glue with no logic to assert: the mount point and test/type shims.
      exclude: [
        "src/main.tsx",
        "src/test-setup.ts",
        "src/vite-env.d.ts",
        "src/**/*.test.{ts,tsx}",
      ],
    },
  },
});
