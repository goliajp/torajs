# torajs-cli

**Workspace-internal** — the `tr` command-line tool. Not a publishable crate.

`torajs-cli` is the binary driver. It wraps `torajs-core` (the
compiler) and `torajs-embed` (in-process JIT) and exposes them as
subcommands.

## Subcommands

```
tr build <file.ts> [-o <out>] [--target wasm32-wasi]
        Compile + link to a native or wasm executable.

tr run   <file.ts>
        JIT compile + run inline.

tr fmt   <file.ts>
        Format source (rustfmt-style, default rules).

tr lint  <file.ts>
        Run lints.

tr cache [size | clean [--max-mb N]]
        Inspect / prune ~/.torajs/cache.

tr debug ast | tokens | ssa | check | ir <file.ts>
        Dump compiler intermediate representations.

tr repl
        Interactive REPL via the embed JIT.

tr lsp
        Language-server protocol stdio loop.
```

Run `tr --help` for the full list at the version you have.

## Pipeline stages (per `Stage` enum in `main.rs`)

```rust
enum Stage {
    Tokens,     // lexer output
    Ast,        // parser output
    Ssa,        // ssa_lower output
    Check,      // typechecker output
    Ir,         // ssa_inkwell LLVM IR
    Build,      // cc / wasm-ld → executable
    Run,        // execute the JIT'd output
    Fmt,        // tr fmt
    Lint,       // tr lint
}
```

## Caches under `~/.torajs/cache`

- Per-fixture `.o` cache, key = `TORAJS_COMPILER_REV` + AST hash +
  import-closure hash.
- Per-`tr run` AOT cache (binary memoization keyed by source SHA-256
  + import closure).
- Default cap 5 GiB; LRU prune via `tr cache clean --max-mb N`.

## Module layout

| Module | Purpose |
| --- | --- |
| `main.rs` | Argument parser + subcommand dispatch + cache machinery |
| `lsp.rs` | Language-server protocol (JSON-RPC over stdio) |
| `lsp_bench.rs` | LSP request benchmarking harness |
| `repl.rs` | REPL state machine |

## Known god-file follow-up

`main.rs` is 1228 LOC (+257 vs the 971-line baseline in
`.claude/rules/common/file-size.md` Known Debt). Phase 1 / P7 ship
sequence accreted subcommands. A follow-up split into per-subcommand
modules (`cmd_build.rs` / `cmd_run.rs` / `cmd_cache.rs` / ...) is
queued post-publish-polish.

## License

Workspace-internal. Depends on `torajs-core` (which depends on
`inkwell` + `llvm-sys`), so not crates.io-publishable.
