# torajs-runtime

**Workspace-internal** — Phase 1 vestige. Not a publishable crate.

This crate's sole remaining contents is the `runtime_libc_bridge.c`
TU that's compiled only under `#ifdef __wasi__`. On native targets
the file compiles to an empty TU; on `wasm32-wasi` it provides 6
`__torajs_libc_*` wrappers (malloc / realloc / memcpy / memmove /
memcmp / free) that take `i64` and pass to libc with implicit
truncation to `size_t` (i32 on wasm32). Needed because the
ssa_inkwell IR emits `call malloc(i64)` but wasi-libc's `size_t`
is i32 and wasm-ld rejects signature mismatches.

## Phase 1 history

This crate previously held 12 `runtime_*.c` files totaling ~12.6
KLOC of C runtime. The Phase 1 architecture rewrite (P3.1 →
P7.i-closer, 2026-05-23 → 2026-05-24) ported every one to a
dedicated Rust sub-crate, deleting the C source as each phase
closed. By P7.i-closer the last business-logic C file
(`runtime_str.c`) was reduced to a 40-line comment-only "FINAL
NUKE" marker; that stub was deleted at A.1 (2026-05-25). All
~12.6 KLOC of business logic now lives in Rust across 13
publishable Layer-1/2/3 stones (`torajs-str` / `_num` / `_bigint`
/ `_arr` / `_dynobj` / `_collections` / `_weak` / `_cycle` /
`_microtask` / `_promise` / `_regex` / `_fetch` / `_date` plus
auxiliary `_fs` / `_meta` / `_process` / `_capture-box`).

## What's left and why

| File | LOC | Reason for keeping |
| --- | ---: | --- |
| `runtime_libc_bridge.c` | 61 | `wasm32-wasi` ABI shim (see top). Port to Rust queued as A.2 follow-up; requires wasm32-wasip1 staticlib build pipeline that's a separate piece of infra. Native target gets a 0-byte object — zero footprint. |

`pub const SOURCES` in `lib.rs` is the (filename, content) list
that `ssa_inkwell` writes + `cc`s into each `tr build` invocation.

## Publishable plan

When the wasm libc bridge is ported to Rust (A.2 follow-up), this
crate becomes empty and can be deleted entirely. Until then it
keeps a tiny scope as the holder for the one remaining C TU.

## License

Workspace-internal. License headers per `Apache-2.0 OR MIT`
following the rest of the torajs workspace, but the crate is
NOT published to crates.io.
