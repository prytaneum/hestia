# frontend

The UI for `hestia-desktop`. A plain client SPA — React 19, TanStack Router + Query, Tailwind v4 — rendered inside a
Tauri v2 webview. There is no Node server at runtime: Tauri opens the dev server in dev and bundles `dist/` in release.

Everything the UI knows comes from the daemon over the shell's one generic
`ipc_call` bridge. See [../docs/architecture/frontends.md](../docs/architecture/frontends.md)
for the design, [../docs/contributing.md](../docs/contributing.md) for the copy-and-adapt recipes,
and [../docs/hooks.md](../docs/hooks.md) for consuming the queries layer.

## Layout

| Path              | What lives there                                               |
|-------------------|----------------------------------------------------------------|
| `src/api/`        | typed daemon calls, one namespace per domain, over `core/`     |
| `src/queries/`    | React Query bindings 1:1 with `api/`, plus the job store       |
| `src/features/`   | one directory per product area                                 |
| `src/components/` | shared chrome, `form/` and the `ui/` primitives                |
| `src/routes/`     | file-based routes; `routeTree.gen.ts` is generated and tracked |
| `src/hooks/`      | app-wide hooks not tied to one feature                         |
| `src/lib/`        | pure helpers — formatting, logging, paths, no React            |
| `src/mock/`       | the browser fixture daemon — dev only, stripped from release   |
| `messages/`       | the message catalogue, one file per root, per locale           |
| `tests/`          | `unit/` and `integration/`, over the harness in `support/`     |

Every feature has the same shape, and a directory only exists once it has
something in it:

```
features/<name>/
├── page.tsx          the route body; detail.tsx for a $id route
├── components/       presentational pieces
├── dialogs/          one file per dialog, named for what it does
├── hooks/            use-*.ts — stateful React
├── lib/              pure helpers, no React
├── tabs/             a detail page's tab bodies
└── <flow>/           a wizard: dialog.tsx · steps/ · its own hooks/ + lib/
```

Every multi-file directory carries an `index.ts` and is imported by directory,
never by file (`@/features/skins/components`); siblings import each other
relatively. Vocabulary two features share lives in `features/shared/`, which is
a leaf — `shared/entry` is what a server and an instance both are, and
`shared/content` is what the browse pages and the entry content tabs resolve
content kinds against. Nothing under `shared/` may import a feature, so the
feature graph stays acyclic.

## Working on it

```bash
bun install
bun run generate:messages   # src/paraglide/ — generated, untracked, imported
bun run dev                 # browser, against the fixture daemon in src/mock
```

`generate:messages` comes first: the app imports from `src/paraglide/`, which is compiled from `messages/` and never
committed, so a typecheck or build before it fails on unresolved imports. For the real thing, `scripts/run.sh --desktop`
from the repo root runs the Tauri shell against a live daemon.

## Checks

The same chain CI runs, in this order:

```bash
bun run check       # biome: lint + format
bun run typecheck   # tsc --noEmit
bun run test        # vitest
bun run build
```

Tests are split by what they touch: `unit/` is pure logic, and `integration/`
renders through `tests/support` — the app's real query client plus the
`src/mock` fixture daemon, so there is no second set of fakes to drift.
