# torajs-anyvalue

[![Crates.io](https://img.shields.io/crates/v/torajs-anyvalue?style=flat-square&logo=rust)](https://crates.io/crates/torajs-anyvalue)
[![docs.rs](https://img.shields.io/docsrs/torajs-anyvalue?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-anyvalue)
[![License](https://img.shields.io/crates/l/torajs-anyvalue?style=flat-square)](#license)

Boxed `Type::Any` value primitives for the [torajs] AOT TypeScript
runtime. Single in-workspace dependency: [`torajs-rc`].

Layer-1 substrate built atop the universal heap-header crate. Used
by every Any-typed slot, Array<Any> element, dynobj bucket value.

## API surface

| Item | Description |
|---|---|
| `AnyBox` | `#[repr(C, align(8))]` 24-byte heap struct: header + tag + value |
| `AnyValue` | Rust enum materialization (Null / Undef / Bool / I64 / F64 / Heap / Unknown) |
| `AnyBox::alloc(tag, value)` | Owned alloc; `rc_inc`s Heap child |
| `AnyBox::drop_owned(ptr)` | Owned destructor; rc_dec + Heap child walk + dealloc |
| `AnyBox::slot_tag()` | `Option<AnySlotTag>` reader |
| `AnyBox::read()` | `AnyValue` materializer |
| `payload_rc_inc(tag, value)` | rc-bump Heap children at slot-copy / dup sites |
| FFI shims | `__torajs_any_box / unbox_tag / unbox_value / payload_rc_inc / any_box_drop` |

## Safety

All allocation / drop methods are `unsafe` end-to-end. Caller
guarantees:

- For `alloc(Heap, ptr)`: `ptr` is null or a valid `*mut HeapHeader`.
- For `drop_owned(p)`: `p` was returned by `AnyBox::alloc` AND no
  other code holds a reference when the refcount transitions to
  zero (the standard rc_dec contract).
- Single-threaded — torajs runtime is single-threaded; concurrent
  mutation of an `AnyBox` is UB.

## Layout invariant

```
offset 0..7  : header   = HeapHeader (refcount, type_tag=ANY_BOX, flags)
offset 8..15 : tag      = i64 (one of AnySlotTag::{Null=0,Bool=1,I64=2,F64=3,Heap=4,Undef=5})
offset 16..23: value    = i64 (inline value or *mut HeapHeader cast)
```

24 bytes 8-aligned. Total size locked across the binary because
ssa_lower IR-emits const-offset reads against this layout.

## License

Licensed under either of

- Apache License, Version 2.0
  ([LICENSE-APACHE](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <https://opensource.org/licenses/MIT>)

at your option.

[torajs]: https://github.com/goliajp/torajs
[`torajs-rc`]: https://crates.io/crates/torajs-rc
