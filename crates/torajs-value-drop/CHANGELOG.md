# Changelog

All notable changes to `torajs-value-drop` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-24

### Added

- Initial crate scaffold extracted from torajs P7.i-drop (commit
  `82d0d41`, 2026-05-24). Replaces the C-side
  `__torajs_value_drop_heap` previously in `runtime_str.c`.
- `__torajs_value_drop_heap(child: *mut c_void)` — reads the
  universal heap-header's `type_tag` at offset +4, dispatches to
  the matching per-type `_drop` extern resolved at `tr build` link
  time against the sibling staticlibs (torajs-str, torajs-arr,
  torajs-bigint, torajs-weak{ref,map,set}, torajs-collections,
  torajs-dynobj, torajs-fetch).
- Fallback arm for tags without a registered `_drop` (Obj / Substr
  / Closure / RegExp / Date / AnyBox): `rc-dec`; `free()` on
  hit-zero.
- `Response` dispatch is `#[cfg(not(target_os = "wasi"))]` —
  libcurl unavailable on WASI, mirrors runtime_str.c's old
  `#ifndef __wasi__` gate.

### Polished (2026-05-25)

- README.md with badges + "Why a dedicated crate" rationale +
  Dispatch table + Quick start + "Behavior" notes.
- CHANGELOG.md (this file) per Keep a Changelog v1.1.0.
- LICENSE-MIT + LICENSE-APACHE dual-license.
- `tests/null_input.rs` — verifies the null-input no-op contract
  (the only invariant testable without a fully-initialized type
  tag table; per-tag dispatch needs the sibling staticlibs which
  are workspace-internal).
- `benches/value_drop.rs` — criterion bench on a fabricated tagged
  heap block to measure dispatch latency (the `type_tag` load +
  match overhead).
- `BUDGETS.md` — per-call dispatch latency budget + binary-size
  contribution.
