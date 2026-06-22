import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { MantineProvider } from "@mantine/core";
import "@mantine/core/styles.css";
import { App } from "./App";
import { createBridgeSource, inMcpApp, setActiveSource } from "./api";
import { seedFromToolResult, type Seed } from "./lib";

/** Boots the UI. Inside an MCP App host we connect the App bridge and route data
 *  through it; standalone we keep the default HTTP source. Either way the same
 *  components render — only the data seam differs (DESIGN §7). A failed bridge
 *  handshake falls back to HTTP so the app still loads. The bridge SDK is loaded
 *  only when embedded, so standalone users never download it. */
async function boot() {
  // A holder (not a bare `let`) so the closure assignment below stays visible to
  // the type checker at the render site.
  const captured: { seed: Seed | null } = { seed: null };

  if (inMcpApp()) {
    try {
      const { App: McpApp } = await import("@modelcontextprotocol/ext-apps");
      const bridge = new McpApp({ name: "joblode", version: "0.1.0" });
      // Register before connect so the host-pushed result of the tool Claude
      // called isn't missed; it seeds the first render (DESIGN §7).
      bridge.ontoolresult = (result) => {
        captured.seed = seedFromToolResult(result.structuredContent) ?? captured.seed;
      };
      await bridge.connect();
      setActiveSource(createBridgeSource(bridge));
    } catch {
      // Stay on the HTTP source — better a degraded app than a blank iframe.
    }
  }

  const root = document.getElementById("root");
  if (root) {
    createRoot(root).render(
      <StrictMode>
        <MantineProvider>
          <App
            initialResults={captured.seed?.results}
            initialScores={captured.seed?.scores}
          />
        </MantineProvider>
      </StrictMode>,
    );
  }
}

void boot();
