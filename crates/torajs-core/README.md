# torajs-core

**Workspace-internal** — the torajs compiler. Not a publishable crate.

`torajs-core` owns the **entire compilation pipeline** that the `tr`
CLI invokes. Largest crate in the workspace (~63 KLOC) — it's the
project's heart.

## Pipeline

```
.ts source
    │
    ▼
 lexer.rs        →  tokens
    │
    ▼
 parser.rs       →  AST (ast.rs)
    │
    ▼
 modules.rs      →  module-resolved AST
    │
    ▼
 check.rs        →  type-checked AST
    │
    ▼
 ssa_lower.rs    →  SSA IR (ssa.rs)
    │
    ▼
 ssa_inkwell.rs  →  LLVM IR via inkwell  →  cc / wasm-ld  →  native or wasm binary
```

## Module table (the largest source files)

| Module | LOC (prod) | Role |
| --- | ---: | --- |
| `ssa_lower.rs` | 28790 | AST → SSA lowering. Visits each AST node, emits SSA instructions, handles closure capture, async/await, exception throw-check insertion, type-narrowing, monomorphization, native error throws, intrinsic dispatch. |
| `ast.rs` | 11546 | AST node types + visitor + debug printing + transforms (desugar_classes / desugar_for_of / etc.). |
| `check.rs` | 7600 | Type checker. Bidirectional with full ES type-system semantics. |
| `parser.rs` | 7134 | Recursive-descent TS parser. |
| `ssa_inkwell.rs` | 4047 | SSA → LLVM IR via inkwell. Owns the libtorajs_*.a staticlib link list, emits `define_*` IR builders for hot-path runtime helpers (Internal linkage + alwaysinline), drives `tr build` cc/wasm-ld invocations. |
| `lexer.rs` | 1198 | Tokenizer (UTF-8 + ES identifier rules). |
| `formatter.rs` | 1136 | `tr fmt` source formatter. |
| `ssa.rs` | 1118 | SSA IR types + builder helpers. |
| `linter.rs` | 684 | `tr lint` linter. |
| `modules.rs` | ~700 | Module resolution + import graph. |

## Other (smaller) modules

`spans.rs`, `messages.rs`, `text.rs`, `hir_pp.rs`, ...

## Embedded staticlib chain

At `tr build` time `ssa_inkwell::compile()` writes the 24 Rust
staticlibs (embedded as `&[u8]` consts assembled at compile time
of `torajs-core` via `build.rs` — see `TORAJS_STATICLIBS` in
`lib.rs`) to per-build tempfiles and passes the paths to cc /
wasm-ld alongside the per-build runtime_libc_bridge.c .o file.

## License

Workspace-internal. License headers per `Apache-2.0 OR MIT`
following the rest of the torajs workspace; the crate is NOT
published to crates.io (depends on `inkwell` + `llvm-sys` which
makes the publish-target story complicated).
