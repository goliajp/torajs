# Changelog

All notable changes to `torajs-rc` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-22

### Added

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

### Notes

- `#![no_std]` + only `core::ffi::c_void`; zero runtime dependencies.
  Dev-dep `criterion` is gated by `[dev-dependencies]`, not present
  in produced binaries.
- API is `unsafe` end-to-end — `rc_inc` / `rc_dec` take raw `*mut
  c_void`, caller guarantees pointer validity + single-threaded
  contract.
- Per-type drop dispatch (`__torajs_value_drop_heap`) is not part of
  this crate; it dispatches by `type_tag` elsewhere.
