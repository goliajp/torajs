# Changelog

All notable changes to `torajs-throw` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-23

### Added

- Initial crate scaffold extracted from `runtime_str.c`'s
  native-error helpers (P2.4-b, 2026-05-23). The C-side
  `__torajs_throw_range_error` / `__torajs_throw_type_error`
  wrappers + the 3-slot factory registry (Error / TypeError /
  RangeError) ported to Rust as a Layer-1 substrate.
- TLS throw-slot machinery: `__torajs_throw_set` /
  `__torajs_throw_check` / `__torajs_throw_take` /
  `__torajs_throw_take_tag`. The "TLS" is currently a `static
  AtomicI64` pair — single-threaded runtime; atomics only for the
  Rust safety story, not for actual concurrent mutation.
- Native-error factory registry: `__torajs_register_native_error`
  + `SLOT_ERROR` / `SLOT_TYPE_ERROR` / `SLOT_RANGE_ERROR` slot
  discriminants matching the C ABI.
- Convenience throwers: `__torajs_throw_range_error(msg)` /
  `__torajs_throw_type_error(msg)` allocate a Str holding the
  message, invoke the registered factory (or fall back to bare-
  string throw), and store the result into the TLS throw slot.

### Polished (2026-05-25)

- README.md with badges + API surface table + design rationale.
- CHANGELOG.md (this file) per Keep a Changelog v1.1.0.
- LICENSE-MIT + LICENSE-APACHE dual-license.
- `tests/throw_slot.rs` — black-box tests for the TLS throw-slot
  API (set + check + take, take_tag preserves tag while take
  zeroes the slot, idempotent take after empty slot).
- `tests/registry.rs` — registry slot read-back tests + null-
  factory edge case.
- `benches/throw.rs` — criterion bench for the happy-path
  throw_check polls (0 throws expected) + the cold-path throw
  set / take / clear cycle.
- `BUDGETS.md` — per-op latency budgets for the happy-path poll
  and the cold-path set/take.
