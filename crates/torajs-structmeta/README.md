# torajs-structmeta

[![Crates.io](https://img.shields.io/crates/v/torajs-structmeta?style=flat-square&logo=rust)](https://crates.io/crates/torajs-structmeta)
[![docs.rs](https://img.shields.io/docsrs/torajs-structmeta?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-structmeta)
[![License](https://img.shields.io/crates/l/torajs-structmeta?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-structmeta?style=flat-square)](https://crates.io/crates/torajs-structmeta)

Struct-layout reflection substrate for the [torajs] AOT TypeScript
runtime. Read-side helpers over the toolchain-emitted
`__torajs_class_layouts` table (W-J Phase A3b wrote the on-disk per-class
field metadata; this crate, **Phase A4**, reads it back at runtime).

The cycle collector (`torajs-cycle`) already consumes the same table for
its child-pointer offsets. This crate adds the **field-name + byte-offset
+ type-tag** read path the reflection ops need:

- `Object.getOwnPropertyDescriptor(o, k)` (Phase B — first user-visible
  unlock)
- `Object.keys` / `values` / `entries` (Phase C)
- `inspect.rs` `Tag::Obj` pretty-printer (Phase D)

## ABI

Mirrors `torajs-link/src/user_class_layouts_layout/types.rs` (the on-disk
emit side) **byte for byte**:

| Region | Layout | Size |
| --- | --- | ---: |
| Outer entry (`StructLayoutEntry`) | `{ u32 n_children, u32 _pad, *const u32 child_offsets, *const u8 field_metadata_ptr }` | 24 |
| Inner array header (`FieldMetaArrayHeader`) | `{ u32 n_fields, u32 _pad }` | 8 |
| Inner field entry (`FieldMeta`) | `{ *const u8 name, u32 name_len, u32 field_byte_offset, u8 type_tag, _pad }` | 24 |

The outer table is indexed by `class_tag - 1` (the `class_tag` is a `u32`
stamped at `+8` on every class / anonymous-struct instance; `0` means
"no layout"). `#[test]` assertions lock `size_of` / `offset_of` against
these numbers so an ABI drift on either side trips a unit-test failure.

## C API

```c
const StructLayoutEntry *__torajs_struct_layout_lookup(uint32_t class_tag);
StrSlice  __torajs_struct_field_name(const StructLayoutEntry *layout, uint32_t idx);
FieldInfo __torajs_struct_field_info(const StructLayoutEntry *layout, uint32_t idx);
uint32_t  __torajs_struct_field_find(const StructLayoutEntry *layout, const uint8_t *name, uint32_t name_len);
```

`field_find` is a linear scan (struct field counts are small — the perfect
hash is a deferred follow-up if a workload ever pushes counts high enough
to matter; reflection is not a hot path).

## License

Apache-2.0 OR MIT.

[torajs]: https://torajs.com
