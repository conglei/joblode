import { formatSalary } from "./lib";

export function App() {
  return (
    <main style={{ font: "15px/1.5 system-ui", maxWidth: 640, margin: "3rem auto", padding: "0 1rem" }}>
      <h1>joblode</h1>
      <p>
        MCP-native job search over the open-jobs dataset — a Rust backend and a React UI that an agent
        drives end to end. Scaffolding in progress; see <code>docs/DESIGN.md</code>.
      </p>
      <p>Example salary format: {formatSalary(120, 180)}</p>
    </main>
  );
}
