# Frontend conventions (TypeScript + React)

How we write the `web/` frontend. The analogue of our Rust bar (`rustfmt`-clean, `clippy -D warnings`):
**strict `tsc`, ESLint-clean, tested, and minimal.** This file is the source of truth for TS/React style;
keep it lean and don't duplicate [DESIGN.md](DESIGN.md) §7 (the "one build, two runtimes" architecture).

The guiding constraint is the same as everywhere in joblode: **the frontend stays thin.** The intelligence
lives in Claude and the Rust server. The UI renders data and captures intent — nothing more. If a component
is growing a brain, the logic probably belongs in the server or in a plain function.

---

## 1. Project structure

Organize by **role**, co-locate by **feature**. The tree is intentionally shallow (≤2 levels under `src/`):

```
web/src/
  main.tsx               # entry: mounts <MantineProvider><App/></MantineProvider>
  App.tsx                # top-level composition + page state
  api.ts                 # data-source adapter (HTTP today; MCP bridge in Phase 5)
  types.ts               # wire types, mirroring crates/joblode-server/src/dto.rs
  lib.ts                 # pure helpers (formatSalary, …) — no React, no I/O
  components/            # presentational + small container components
    FilterSidebar.tsx
    ResultsTable.tsx
    JobDrawer.tsx
  *.test.tsx / *.test.ts # tests sit next to the code they cover
```

Rules:

- **Co-locate tests** with their subject (`App.test.tsx` next to `App.tsx`), not in a separate `__tests__/` tree.
- **One component per file**, named the same as the file. A file exports one primary thing.
- **Pure, framework-free logic goes in `lib.ts`** (or a sibling module), so it's trivially unit-testable
  without rendering. Parsing, formatting, sorting, validation — deterministic code, per CLAUDE.md.
- Don't add `pages/`, `hooks/`, `services/`, state-library folders, or barrel `index.ts` files until there's
  real weight to justify them. Premature structure is the same smell as premature abstraction.

## 2. File & naming conventions

- **Files:** `PascalCase.tsx` for components (`FilterSidebar.tsx`); `camelCase.ts` for non-component modules
  (`api.ts`, `lib.ts`, `types.ts`). This matches the export each file holds.
- **Components:** `PascalCase`. **Functions, variables, props:** `camelCase`. **Types/interfaces:** `PascalCase`.
- **Booleans** read as predicates: `loading`, `isOpen`, `hasResults`.
- **Event handlers:** `handleX` for the implementation, `onX` for the prop that receives one
  (`onSearch={handleSearch}`).
- Mirror server field names **verbatim** on the wire (`salary_min_k`, `remote_scope`) so `types.ts` is a
  faithful image of `dto.rs`. Don't camel-case-rename wire fields — a 1:1 mapping is worth the snake_case.

## 3. TypeScript

Strict mode is on (`strict`, `noUnusedLocals`, `noUnusedParameters`, `verbatimModuleSyntax`,
`isolatedModules`). Work with it, not around it.

- **No `any`.** Use `unknown` at boundaries (e.g. `catch (cause: unknown)`) and narrow. No
  non-null `!` assertions or `as` casts to silence the checker — fix the type instead. The only routine
  `as` is asserting a `fetch().json()` to its known response type at the adapter boundary (see §5).
- **`interface` for object/props shapes; `type` for unions, intersections, and aliases.** Both are fine;
  this split keeps props extendable and unions expressive. Don't agonize over it.
- **Type-only imports are explicit:** `import type { Job } from "./types"`. `verbatimModuleSyntax` enforces
  this — it keeps types out of the JS output and the bundle.
- **Let inference work.** Annotate function parameters, props, and exported/public signatures; let local
  variables and component return types infer. Don't write `: JSX.Element` on every component.
- **No `React.FC`.** Declare components as plain functions taking a typed props object — the modern,
  cheatsheet-recommended form (`React.FC`'s implicit `children` and generics friction aren't worth it):
  ```tsx
  interface ResultsTableProps {
    rows: JobSummary[];
    onSelect: (id: string) => void;
  }
  export function ResultsTable({ rows, onSelect }: ResultsTableProps) { … }
  ```
- **Discriminated unions over boolean soup** for mutually exclusive states. Prefer one
  `{ status: "idle" | "loading" | "error" | "ready"; … }` to a tangle of `loading`/`error`/`data` flags
  that can contradict each other, once a component has more than a couple of states.
- **Avoid TS `enum`** (it emits runtime code and has surprising semantics); use a union of string literals
  or `as const` objects.

## 4. React

- **Function components and hooks only.** No class components.
- **Rules of Hooks:** call hooks unconditionally at the top level; never in conditions, loops, or after an
  early `return`. The ESLint `react-hooks` rules enforce this — keep them green.
- **Lift state to the lowest common owner.** Page-level state (search results, the selected row id) lives in
  `App.tsx`; a component owns only what's purely its own (e.g. the filter form's draft inputs in
  `FilterSidebar`). Pass data down as props and changes up as callbacks.
- **Keep effects honest.** An effect is for synchronizing with something external (a fetch, a subscription).
  Give it a correct dependency array, and guard async effects against races — capture an `active` flag and
  ignore the result if it flipped (see `JobDrawer`'s fetch-on-`jobId`). Don't use effects to derive state you
  can compute during render.
- **Derive, don't mirror.** Compute values from props/state inline; don't copy a prop into state and try to
  keep them in sync.
- **Keys are stable ids** (`row.id`), never the array index.
- **Optimize only with evidence.** Don't reach for `useMemo`/`useCallback`/`memo` by default; add them when a
  profile shows a real cost. (React 19's compiler reduces the need further.) Clarity first.
- **Accessibility is non-negotiable:** real `<button>`/`<a>` for actions and links, labelled inputs, and
  prefer Mantine components that ship correct roles/ARIA. The payoff is double: usable by everyone, and
  testable by accessible role (§7).

## 5. Data fetching & the adapter boundary

All server I/O goes through **`api.ts`** — the single data-source seam. Components never call `fetch`
directly; they call `searchJobs` / `getJob`. This is what lets Phase 5 swap the HTTP adapter for the MCP App
bridge without touching a single component (DESIGN §7).

- The adapter owns: URL construction, request shape, non-2xx → thrown `Error`, and the one `as`-assertion of
  the parsed JSON to its wire type.
- Components own: calling the adapter, and rendering **all three outcomes — loading, error, and empty** — not
  just the happy path. Every fetch surface shows a loader, surfaces failures, and has an empty state.
- Keep `types.ts` in lockstep with `crates/joblode-server/src/dto.rs`. When the server's wire shape changes,
  update `types.ts` in the same PR.

## 6. Styling — Mantine

Mantine is our design system (chosen Phase 3; see DESIGN §7). Use it; don't hand-roll UI.

- **Reach for a Mantine component before writing markup + CSS.** Layout via `AppShell`/`Stack`/`Group`,
  inputs via `TextInput`/`TagsInput`/`NumberInput`, etc. We get accessibility, theming, and dark mode for free.
- **Theme, don't inline.** Prefer Mantine props (`gap`, `c`, `fw`, `size`) and the theme over ad-hoc `style={}`.
  Reserve inline `style` for one-off layout that has no prop (e.g. `cursor: "pointer"` on a row).
- **No second UI library and no Tailwind.** One system. If something's missing, compose Mantine primitives.
- Import Mantine's CSS once in `main.tsx` (`@mantine/core/styles.css`) and wrap the app in a single
  `MantineProvider`.

## 7. Testing

Vitest + React Testing Library + `@testing-library/user-event`, jsdom environment. We follow Testing
Library's guiding principle: **test behavior the way a user experiences it, not implementation details.**

- **Query by accessible role/label/text**, in that priority order (`getByRole("button", { name: "Search" })`).
  Avoid `data-testid` and never reach into component internals or state.
- **Drive interactions with `user-event`**, not `fireEvent` — it models real user behavior (focus, typing,
  click sequencing).
- **`findBy*` for async appearance**; `waitFor` for async assertions. Don't sprinkle arbitrary timeouts.
- **Mock at the adapter seam** (`vi.mock("./api")`), not `global.fetch`. Tests then exercise real component
  logic against controlled data — the same seam the bridge will swap at.
- **Cover the flow, plus the unhappy paths.** Pure helpers get focused unit tests (`lib.test.ts`); the app gets
  a smoke test of the key journey (search → rows → open drawer, in `App.test.tsx`). Assert loading/error/empty
  states, not only success.
- jsdom lacks the layout APIs Mantine touches (`matchMedia`, `ResizeObserver`, `scrollIntoView`); the shims
  live once in `src/test-setup.ts`.

## 8. Tooling & quality gates

Run before pushing — CI gates on all of it, orchestrated by Turbo:

```bash
pnpm turbo run lint typecheck test build
```

- **lint** — ESLint flat config (`@eslint/js` + `typescript-eslint` recommended). Warning-free, like clippy.
- **typecheck** — `tsc --noEmit` under strict. Zero errors.
- **test** — `vitest run --coverage`.
- **build** — `tsc --noEmit && vite build` must succeed.

Don't disable a rule to get green; fix the cause. If a rule is genuinely wrong for the repo, change the
config deliberately (its own small PR), not with a scattered `eslint-disable`.

---

### Sources

Current best practice these conventions are grounded in:

- [React TypeScript Cheatsheet](https://react-typescript-cheatsheet.netlify.app/) — function components over
  `React.FC`, props typing, type-only imports, discriminated unions.
- [Testing Library guiding principles](https://testing-library.com/docs/guiding-principles/) — query by
  accessibility, avoid implementation details.
- [React docs — Rules of Hooks](https://react.dev/reference/rules/rules-of-hooks) and
  [You Might Not Need an Effect](https://react.dev/learn/you-might-not-need-an-effect).
- [Recommended React folder structure, 2025](https://dev.to/pramod_boda/recommended-folder-structure-for-react-2025-48mc).
