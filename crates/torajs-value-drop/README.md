# torajs-value-drop

[![Crates.io](https://img.shields.io/crates/v/torajs-value-drop?style=flat-square&logo=rust)](https://crates.io/crates/torajs-value-drop)
[![docs.rs](https://img.shields.io/docsrs/torajs-value-drop?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-value-drop)
[![License](https://img.shields.io/crates/l/torajs-value-drop?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-value-drop?style=flat-square)](https://crates.io/crates/torajs-value-drop)

Universal heap-typed drop dispatch — `__torajs_value_drop_heap(child)`
reads a universal heap-header's `type_tag` at offset +4, switches on
the discriminant, and dispatches to the matching per-type `_drop`
extern. Single Cargo dep (`torajs-rc` for the shared `Tag` enum +
refcount-dec primitive).

## Why a dedicated crate

The crate's whole job is one `match` on a `u16` discriminant. So why
own a crate file? Because the alternative (folding into `torajs-rc`)
would force `torajs-rc` to depend on *every* tagged-type substrate
crate (`torajs-str`, `torajs-arr`, `torajs-dynobj`, ...) — which
inverts the dependency direction. Putting the dispatch in its own
crate that resolves all `__torajs_<x>_drop` externs at `tr build`
link time keeps the dependency graph clean.

## Dispatch table

| `Tag` discriminant | Provider crate | Drop fn |
| --- | --- | --- |
| `Str` | torajs-str | `__torajs_str_drop` |
| `Arr` | torajs-arr | `__torajs_arr_drop` |
| `Response` (non-WASI) | torajs-fetch | `__torajs_response_drop` |
| `BigInt` | torajs-bigint | rc-dec → `__torajs_bigint_drop` |
| `WeakRef` / `WeakMap` / `WeakSet` | torajs-weak | `__torajs_weak{ref,map,set}_drop` |
| `Map` / `MapIter` | torajs-collections | `__torajs_map_drop` / `__torajs_map_iter_drop` |
| `ArrIter` | torajs-arr | `__torajs_arr_iter_drop` |
| `DynObj` | torajs-dynobj | `__torajs_dynobj_drop` |
| _other_ | (fallback) | `__torajs_rc_dec` → `free()` on hit-zero |

The fallback covers `Obj` / `Substr` / `Closure` / `RegExp` / `Date` /
`AnyBox` — these don't currently expose a `_drop` extern (they're
either pure-pointer wrappers that need no destructor, or their inner
refcount walks happen at the call site in array / dynobj element
walks).

## Quick start

```rust
use torajs_value_drop::__torajs_value_drop_heap;

// At the end of an Array<Any>-element scope, when the slot is
// known to be a Heap-tagged child:
# fn _example(slot_ptr: *mut core::ffi::c_void) {
unsafe { __torajs_value_drop_heap(slot_ptr) };
# }
// dispatched on slot_ptr's type_tag → per-type _drop, or fallback
// rc-dec + free.
```

## Behavior

- `NULL` input is a no-op (allows raw caller paths to skip a null
  check).
- Unknown / out-of-range `type_tag` falls into the fallback arm
  (rc-dec; `free` on hit-zero). Won't crash on a corrupted header,
  but the inner-ref walks won't happen — diagnose at the producer
  side.
- The crate does not own the inner-ref walking semantics. Callers
  that drop a tagged-`Heap` value with nested heap children must
  ensure those children are drained before this dispatch.

## License

Dual-licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

[torajs]: https://torajs.com
