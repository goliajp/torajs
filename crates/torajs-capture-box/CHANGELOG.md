# Changelog

All notable changes to `torajs-capture-box` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-24

### Added

- Initial release.
- `__torajs_capture_box_alloc(init_value: i64) -> *mut c_void` —
  allocates 16 bytes (8-byte aligned), writes `init_value` at the
  value slot (`base + 8`), returns the value-slot pointer.
- `__torajs_capture_box_inc(slot: *mut c_void)` — increments the
  refcount at `slot - 8`. Null-tolerant (no-op on null input).
- `__torajs_capture_box_drop(slot: *mut c_void)` — decrements the
  refcount; frees the underlying allocation when it hits zero.
  Defensive at-zero free covers the "heap-promoted but never
  captured" edge case. Null-tolerant.
