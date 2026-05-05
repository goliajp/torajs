# torajs VS Code extension

Minimal Language Server client for the [torajs](https://torajs.com)
runtime. Spawns `tr lsp` to provide diagnostics, hover, and goto-def
for TypeScript files.

## Status

**Preview.** Tracks `v0.3 #5` of the torajs roadmap. Working features:

- Diagnostics — typecheck errors from `check.rs` (anchored at file:1:1
  for now; per-error spans land in L-2.b).
- Hover — inferred type of the smallest expression at the cursor.
- Goto-def — top-level `function` / `class` / `type` / `let` / `const`
  declarations (no scope handling yet; same-name shadows resolve to
  the first occurrence).

Not yet:

- Local-binding goto-def (needs scope-aware symbol table).
- Member / method goto-def (needs class table integration).
- Cross-file resolution.
- Autocomplete / IntelliSense.
- Refactoring (rename / extract).

## Build

```bash
cd web/torajs-vscode
bun install
bun run compile
bunx vsce package --no-dependencies
```

Outputs `torajs-<version>.vsix`.

## Install locally

```bash
code --install-extension torajs-0.1.0.vsix
```

Set the `tr` binary path via `torajs.path` in VS Code settings if `tr`
isn't on `PATH`.
