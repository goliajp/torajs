# Changelog

All notable changes to `torajs-anyvalue` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added — strict equality

- `AnyValue::strict_eq(self, other) -> bool` — idiomatic Rust API
  for JS spec §7.2.13 strict equality. NaN-aware (NaN !== NaN),
  zero-aware (+0 === -0). Heap path delegates to byte-eq via
  `__torajs_str_eq` when both sides are Tag::Str, pointer
  identity otherwise.
- FFI shim `__torajs_any_any_strict_eq(l, r)` — Any === Any (box
  vs box). Both-null-true, one-null-false short-circuits.
- FFI shim `__torajs_any_strict_eq(box, rhs_tag, rhs_value)` —
  Any === concrete; the SSA layer packs the concrete operand
  inline rather than allocating a fresh box per compare site.

## [0.1.0] - 2026-05-22

### Added

- Initial release. Replaces the previous C implementation of
  `__torajs_any_box` / `__torajs_any_unbox_tag` /
  `__torajs_any_unbox_value` / `__torajs_any_payload_rc_inc` /
  `__torajs_any_box_drop` with a pure-Rust crate at byte-identical
  ABI.
- `AnyBox` — `#[repr(C, align(8))]` struct, 24 bytes; mirrors the
  C heap layout `{ header: HeapHeader, tag: i64, value: i64 }`.
- `AnyValue` — Rust enum materialization of the boxed payload
  (Null / Undef / Bool(bool) / I64(i64) / F64(f64) /
  Heap(Option<NonNull<HeapHeader>>) / Unknown). Read-only view
  for downstream Rust `match` callers.
- `AnyBox::alloc(tag, value) -> NonNull<AnyBox>` — owned alloc;
  `rc_inc`s the Heap child if `tag == Heap`.
- `AnyBox::drop_owned(ptr)` — owned destructor; static-literal
  bypass, rc_dec, Heap child drop, then `dealloc` the 24-byte
  block.
- `AnyBox::slot_tag() -> Option<AnySlotTag>` — type-safe tag
  reader.
- `AnyBox::read() -> AnyValue` — materialize the boxed payload.
- `payload_rc_inc(tag, value)` — free fn for the slot-copy /
  bucket-dup pattern; no-op on inline tags, `rc_inc` on Heap.
- FFI shims `__torajs_any_box / __torajs_any_unbox_tag /
  __torajs_any_unbox_value / __torajs_any_payload_rc_inc /
  __torajs_any_box_drop`: thin extern "C" wrappers preserving
  exact ABI for compiler-emitted IR.

### Notes

- Depends only on `torajs-rc` (in-workspace); no external runtime
  deps.
- `#[lib] crate-type = ["rlib", "staticlib"]` — the staticlib is
  embedded into `tr`.
- `__torajs_value_drop_heap` is a separate extern "C" function
  that `AnyBox::drop_owned` calls to tear down Heap-tagged
  children.
