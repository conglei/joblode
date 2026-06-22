/// <reference types="node" />
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { viteSingleFile } from "vite-plugin-singlefile";

// The MCP App bundle: the same React app, inlined into one self-contained
// `index.html` (JS + CSS) so the host can serve it as the `ui://` resource and
// render it in a deny-by-default sandboxed iframe (DESIGN §7). The standalone web
// build (multi-file) stays in `vite.config.ts`; this one only changes packaging.
export default defineConfig({
  plugins: [react(), viteSingleFile()],
  build: {
    outDir: "dist-app",
  },
});
