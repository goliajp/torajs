# Changelog

All notable changes to `torajs-rc` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-22

### Added

- Initial crate, Layer 1 of the torajs architecture rewrite (see
  `docs/architecture-rewrite.md`). Replaces the C-side
  `__torajs_rc_inc` / `__torajs_rc_dec` / `__torajs_heap_header_t`
  definitions in `crates/torajs-runtime/src/runtime_str.c:138-272`
  byte-for-byte; same symbol names, same ABI, same calling
  convention so `ssa_lower` IR emit is unchanged.
- `HeapHeader` — `#[repr(C, align(8))]` `{ refcount: u32, type_tag:
  u16, flags: u16 }`. The universal 8-byte header at offset 0 of
  every refcounted heap value.
- `__torajs_rc_inc` / `__torajs_rc_dec` — `#[no_mangle] #[inline]
  pub unsafe extern "C" fn`. Non-atomic (single-threaded runtime
  invariant). NULL pass-through, FLAG_STATIC_LITERAL bypass,
  WeakRef-on-zero hook to runtime_weakref.c.
- Tag constants (`TAG_STR` through `TAG_ARR_ITER`, 18 entries) and
  flag constants (`FLAG_SPLIT_BLOCK` / `FLAG_STATIC_LITERAL` /
  `FLAG_ARR_ANY` / `FLAG_FROZEN` / `FLAG_BUFFERED` / cycle-collector
  `COLOR_*`). Any-slot tags (`ANY_NULL` / `ANY_BOOL` / `ANY_I64` /
  `ANY_F64` / `ANY_HEAP` / `ANY_UNDEF`) for 16-byte
  `Array<Any>` slots.
- 9 unit tests covering layout (size / align / field offsets),
  constant value parity vs the C `#define`s, null pass-through,
  hit-zero signaling, STATIC_LITERAL bypass (inc + dec), and
  balanced inc/dec invariant.
- 3 criterion micro-benches under `benches/rc.rs`:
  `inc_dec_pair`, `inc_null_passthrough`, `inc_static_literal_bypass`.
- `tests/perf_gate.rs` regression gate: 1M iter loops at 20 ms /
  10 ms budgets (≈ 10× headroom over observed P95).

### Notes

- `#![no_std]` + only `core::ffi::c_void`; zero runtime deps. Matches
  vision item #4 (0 deps). Dev-dep `criterion` is workspace-shared
  and gated by `[dev-dependencies]`, not present in produced binaries.
- API is `unsafe` end-to-end — `rc_inc` / `rc_dec` take raw `*mut
  c_void`, caller guarantees pointer validity + single-threaded
  contract.
- Drop dispatch (`__torajs_value_drop_heap`) intentionally stays in
  the C glue (`runtime_str.c:1371-1444`) for this first sub-step —
  it currently `switch`es to 18 per-type drop fns each provided by
  a different runtime file, and porting all of those is the work of
  P3..P6 (per `docs/architecture-rewrite.md` rollout). Once those
  Rust crates land, the dispatch will become a registry indexed by
  `type_tag` and migrate to `torajs-rc` (or its own glue crate),
  per the Layer-1 design intent.
