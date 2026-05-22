# Changelog

All notable changes to `torajs-anyvalue` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-22

### Added

- Initial crate, Layer-1 substrate of the torajs architecture
  rewrite (see `docs/architecture-rewrite.md`). Replaces the
  C-side `__torajs_any_box` / `__torajs_any_unbox_tag` /
  `__torajs_any_unbox_value` / `__torajs_any_payload_rc_inc` /
  `__torajs_any_box_drop` in
  `crates/torajs-runtime/src/runtime_str.c:1663-2284` with a
  pure-Rust crate at byte-identical ABI.
- `AnyBox` — `#[repr(C, align(8))]` struct, 24 bytes; mirrors the
  C heap layout `{ header: HeapHeader, tag: i64, value: i64 }`.
- `AnyValue` — Rust enum materialization of the boxed payload
  (Null / Undef / Bool(bool) / I64(i64) / F64(f64) /
  Heap(Option<NonNull<HeapHeader>>) / Unknown). Read-only view
  for downstream Rust `match` callers.
- `AnyBox::alloc(tag, value) -> NonNull<AnyBox>` — owned alloc;
  `rc_inc`s the Heap child if `tag == Heap`.
- `AnyBox::drop_owned(ptr)` — owned destructor; static-literal
  bypass, rc_dec, Heap child drop via the C-side per-type
  dispatcher (still in `runtime_str.c::value_drop_heap` pre-P3),
  then `dealloc` the 24-byte block.
- `AnyBox::slot_tag() -> Option<AnySlotTag>` — type-safe tag
  reader.
- `AnyBox::read() -> AnyValue` — materialize the boxed payload.
- `payload_rc_inc(tag, value)` — free fn for the slot-copy /
  bucket-dup pattern; no-op on inline tags, `rc_inc` on Heap.
- FFI shims `__torajs_any_box / __torajs_any_unbox_tag /
  __torajs_any_unbox_value / __torajs_any_payload_rc_inc /
  __torajs_any_box_drop`: thin extern "C" wrappers preserving
  exact ABI for ssa_lower-emitted IR.
- 13 unit tests covering layout invariants, tag round-trips,
  bitcast through F64, Heap rc_inc on alloc, static-literal
  bypass, FFI shim behavior.
- 3 criterion benches: `box_unbox_i64`, `box_heap_alloc_drop`,
  `payload_rc_inc_inline_tag`.
- `tests/perf_gate.rs` regression gate: 100k alloc/drop pair at
  50 ms (≈10× headroom).

### Notes

- Depends only on `torajs-rc` (in-workspace, Layer-0/1
  substrate); no external runtime deps. Matches vision item #4
  (0 deps).
- `#[lib] crate-type = ["rlib", "staticlib"]` — staticlib is
  embedded into `tr` via the build infra in `torajs-core/build.rs`
  + `lib.rs::TORAJS_ANYVALUE_STATICLIB` const + `ssa_inkwell`
  link-step.
- `__torajs_value_drop_heap` is still a C-side function (lives
  in runtime_str.c); torajs-anyvalue calls it `extern "C"` from
  `AnyBox::drop_owned` to teardown Heap-tagged children. Rewrite
  of value_drop_heap dispatch lives in P3 — at that point the
  cross-language call becomes Rust-to-Rust.
