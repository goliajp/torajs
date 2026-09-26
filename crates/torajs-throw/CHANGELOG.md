# Changelog

All notable changes to `torajs-throw` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-23

### Added

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
