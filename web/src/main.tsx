import { StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { MantineProvider } from "@mantine/core";
import "@mantine/core/styles.css";
import { App } from "./App";
import { McpApp } from "./McpApp";
import { createBridgeSource, inMcpApp, setActiveSource } from "./api";
import { seedFromToolResult, type Seed } from "./lib";

function renderRoot(node: ReactNode) {
  const root = document.getElementById("root");
  if (root) {
    createRoot(root).render(
      <StrictMode>
        <MantineProvider>{node}</MantineProvider>
      </StrictMode>,
    );
  }
}

/** Boots the UI (DESIGN §7). Inside an MCP App host: connect the bridge, route data
 *  through it, and render the compact {@link McpApp} seeded from the tool result
 *  Claude pushed. Standalone: render the full {@link App} over the HTTP API. A
 *  failed handshake falls back to the standalone app. The bridge SDK is imported
 *  only when embedded, so standalone users never download it. */
async function boot() {
  if (inMcpApp()) {
    try {
      const { App: AppBridge } = await import("@modelcontextprotocol/ext-apps");
      const bridge = new AppBridge({ name: "joblode", version: "0.1.0" });

      // Register before connect so the result that opened the app isn't missed;
      // forward results to the mounted component (initial seed + later searches).
      const captured: { seed: Seed | null } = { seed: null };
      let pushSeed: (seed: Seed) => void = () => {};
      bridge.ontoolresult = (result) => {
        const seed = seedFromToolResult(result.structuredContent);
        if (seed) {
          captured.seed = seed;
          pushSeed(seed);
        }
      };

      await bridge.connect();
      setActiveSource(createBridgeSource(bridge));
      renderRoot(
        <McpApp
          initial={captured.seed}
          subscribe={(onSeed) => {
            pushSeed = onSeed;
          }}
        />,
      );
      return;
    } catch {
      // Fall through to the standalone app — better than a blank iframe.
    }
  }

  renderRoot(<App />);
}

void boot();
