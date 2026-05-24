# torajs-capture-box

[![Crates.io](https://img.shields.io/crates/v/torajs-capture-box?style=flat-square&logo=rust)](https://crates.io/crates/torajs-capture-box)
[![docs.rs](https://img.shields.io/docsrs/torajs-capture-box?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-capture-box)
[![License](https://img.shields.io/crates/l/torajs-capture-box?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-capture-box?style=flat-square)](https://crates.io/crates/torajs-capture-box)

Refcounted 16-byte heap box for escape-captured `Copy`-typed `let`
slots — designed for an AOT compiler's closure runtime, where a stack
local that gets captured by a closure needs to be promoted to the
heap with shared ownership semantics. 0 Cargo deps. 3 `extern "C"`
ABI fns: `alloc` / `inc` / `drop`.

Extracted from the [torajs] AOT TypeScript runtime as **P6.5**
(commit `c1b9e48`, 2026-05-24) — when `runtime_promise.c` was ported
to `torajs-promise`, the orthogonal 3-fn capture-box helper carved
out into its own standalone Rust crate (distinct layout from
`torajs-rc`'s universal heap header, which is `refcount u32 +
type_tag u16 + flags u16 = 8 B`; capture-box uses `refcount u64 +
i64 value = 16 B` because the box is value-typed and doesn't need
tag dispatch).

## Layout (16 bytes total)

```text
  base + 0  : refcount u64  (rc starts at 0 — see "Why rc=0 initial")
  base + 8  : i64 value     (Number / Bool widened / pointer cast / ...)
```

The `alloc` / `inc` / `drop` API takes pointer-to-`base+8` (the value
slot). Callers thread that pointer and treat it as `*mut i64` for
their `Load` / `Store` IR sites; the refcount bookkeeping steps back
8 bytes inside the helpers. This way, the AOT compiler's existing
`Load i64 at slot+0` / `Store i64 at slot+0` codegen patterns stay
unchanged when a `let` is heap-promoted.

## Quick start

```rust
use torajs_capture_box::{
    __torajs_capture_box_alloc, __torajs_capture_box_drop, __torajs_capture_box_inc,
};

// Allocate a fresh box holding value 42; refcount starts at 0.
let slot = __torajs_capture_box_alloc(42);
assert!(!slot.is_null());

// Read/write through the slot pointer — same shape as a stack `let`
// of `i64`.
let v = unsafe { *(slot as *const i64) };
assert_eq!(v, 42);

// Each closure construction that captures this slot does `inc`.
// Each closure-env drop does `drop`.
unsafe {
    __torajs_capture_box_inc(slot);
    __torajs_capture_box_inc(slot);
    // rc = 2 now; the box stays alive while either closure holds it.
    __torajs_capture_box_drop(slot);
    __torajs_capture_box_drop(slot);
    // rc back to 0; the box is freed.
}
```

## Why rc=0 initial state

A `let` that gets heap-promoted (because some path captures it) but
that the runtime never actually captures still must be freed. The
ABI for that: the heap-promoted slot's owner-side scope-end emits a
`__torajs_capture_box_drop` call regardless of whether `inc` ever
ran. The drop fn checks `rc == 0` and frees on that path, covering
the "promoted but unused" edge case without leaking.

## What it does NOT do

- **No multi-threading.** torajs is single-threaded today (matching
  JS's event-loop model). When threading lands the refcount needs to
  become atomic; the current `u64` write is plain.
- **No type erasure across boxes.** The value slot is always `i64`-
  shaped: caller widens / narrows / casts. If you need
  `Box<dyn Any>`, this isn't the crate.
- **No drop callback for non-Copy contents.** A `let` of a
  refcounted heap pointer (e.g. a string) is captured by raw
  pointer; the *string's* refcount is bumped by `torajs-rc`, not by
  this box. The box stores the pointer itself as an `i64`. The
  promotion is for the *slot* (so all captures share one slot),
  not for the slot's contents' lifecycle.

## License

Dual-licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

[torajs]: https://torajs.com
