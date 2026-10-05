# torajs-core

**Workspace-internal** — the torajs compiler. Not a publishable crate.

`torajs-core` owns the **entire compilation pipeline** that the `tr`
CLI invokes.

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

## Module table

| Module | Role |
| --- | --- |
| `ssa_lower.rs` | AST → SSA lowering. Visits each AST node, emits SSA instructions, handles closure capture, async/await, exception throw-check insertion, type-narrowing, monomorphization, native error throws, intrinsic dispatch. |
| `ast.rs` | AST node types + visitor + debug printing + transforms (desugar_classes / desugar_for_of / etc.). |
| `check.rs` | Type checker. Bidirectional with full ES type-system semantics. |
| `parser.rs` | Recursive-descent TS parser. |
| `ssa_inkwell.rs` | SSA → LLVM IR via inkwell. Owns the staticlib link list, emits IR builders for hot-path runtime helpers, drives `tr build` cc/wasm-ld invocations. |
| `lexer.rs` | Tokenizer (UTF-8 + ES identifier rules). |
| `formatter.rs` | `tr fmt` source formatter. |
| `ssa.rs` | SSA IR types + builder helpers. |
| `linter.rs` | `tr lint` linter. |
| `modules.rs` | Module resolution + import graph. |

## Other (smaller) modules

`spans.rs`, `messages.rs`, `text.rs`, `hir_pp.rs`, ...

## Embedded staticlib chain

At `tr build` time the in-house linker reads the runtime image
(the Rust staticlibs preprocessed at compile time of `torajs-core`
— see `TORAJS_RUNTIME_IMAGE` in `staticlibs.rs`, built by
`torajs-rtimage-macro` from the paths `build.rs` emits).

## License

Workspace-internal. License headers per `Apache-2.0 OR MIT`
following the rest of the torajs workspace; the crate is NOT
published to crates.io (depends on `inkwell` + `llvm-sys` which
makes the publish-target story complicated).
