# Changelog

All notable changes to `torajs-capture-box` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-24

### Added

- Initial crate scaffold extracted from torajs P6.5 (commit
  `c1b9e48`, 2026-05-24). `runtime_promise.c` ported to
  `torajs-promise` had a 3-fn capture-box helper (75 LOC) carved
  out to its own `runtime_capture_box.c`; this commit ports
  that to a standalone Rust crate so Phase 1 (pure rust 重构)
  doesn't leave a stray small C TU in tree.
- `__torajs_capture_box_alloc(init_value: i64) -> *mut c_void` —
  allocates 16 bytes (8-byte aligned), writes `init_value` at the
  value slot (`base + 8`), returns the value-slot pointer.
- `__torajs_capture_box_inc(slot: *mut c_void)` — increments the
  refcount at `slot - 8`. Null-tolerant (no-op on null input).
- `__torajs_capture_box_drop(slot: *mut c_void)` — decrements the
  refcount; frees the underlying allocation when it hits zero.
  Defensive at-zero free covers the "heap-promoted but never
  captured" edge case. Null-tolerant.

### Polished (2026-05-25)

- README.md with badges + Quick start + "What it does NOT do"
  delimiter.
- CHANGELOG.md (this file) per Keep a Changelog v1.1.0.
- LICENSE-MIT + LICENSE-APACHE dual-license.
- `tests/lifecycle.rs` — integration tests covering refcount
  monotonicity, drop-after-multiple-incs, and value-slot 8-byte
  alignment invariant.
- `benches/capture_box.rs` — criterion benches for alloc-inc-drop
  cycle latency (the hot path: every closure construction over a
  captured let runs through one of these per slot).
- `BUDGETS.md` — per-op latency budgets with 5× headroom.
