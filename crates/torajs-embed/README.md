# torajs-embed

**Workspace-internal** — in-process JIT + embed API. Not a publishable crate.

`torajs-embed` is the **embed surface** for host applications that
want to compile + run TS source in-process (without shelling out to
`tr`). It exposes a `compile_and_run(source)` API that goes:

```
host calls embed::compile_and_run(src)
    │
    ▼
 torajs-core: lex → parse → check → ssa → lower → ssa_inkwell
    │
    ▼
 inkwell ORC JIT: link torajs-* staticlibs in-memory
    │
    ▼
 user fn pointer returned to host
```

Currently scaffolded (~330 LOC); the full ORC JIT integration is
queued (T-22 / V3-16 in the legacy roadmap).

## Crate types

- `rlib` — for workspace consumers
- `staticlib` — for non-Rust host integration (currently unused)
- `cdylib` — for dlopen host integration (currently unused)

## Surface (current scaffold)

```rust
pub fn compile_and_run(src: &str) -> Result<Output, EmbedError>;
```

Output captures stdout / stderr + exit code; the in-process JIT
is fenced by:
- `wasmtime` fuel limit (CPU budget cap)
- Wall-clock timeout
- Tempdir per request (file I/O sandbox)

## What's NOT shipped yet

- **Real ORC JIT path**: today the embed implementation shells out
  to `tr` instead of using inkwell's ORC JIT. Direct JIT lands when
  V3-16 ships.
- **Symbol-import API**: host-provided fn pointers callable from TS
  code. Stub only.
- **Multi-shot reuse**: each `compile_and_run` is currently a fresh
  module. Long-lived JIT contexts queued.

## License

Workspace-internal. Depends on `torajs-core` (inkwell-dependent) so
not crates.io-publishable as-is.
