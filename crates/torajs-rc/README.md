# torajs-rc

[![Crates.io](https://img.shields.io/crates/v/torajs-rc?style=flat-square&logo=rust)](https://crates.io/crates/torajs-rc)
[![docs.rs](https://img.shields.io/docsrs/torajs-rc?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-rc)
[![License](https://img.shields.io/crates/l/torajs-rc?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-rc?style=flat-square)](https://crates.io/crates/torajs-rc)

Universal heap-object header + non-atomic refcount primitives for the
[torajs] AOT TypeScript runtime. `no_std`. Zero runtime dependencies.

Layer 1 of the torajs runtime crate stack (sits above [`torajs-pool`]
allocator; below the per-type containers `torajs-str` / `torajs-arr` /
… that the rewrite lands later). See [`docs/architecture-rewrite.md`]
for the full layered plan.

## What this provides

- **`HeapHeader`** — `#[repr(C, align(8))]` `{ refcount: u32, type_tag:
  u16, flags: u16 }`. The 8-byte universal header that sits at offset
  0 of every refcounted torajs heap value.
- **`__torajs_rc_inc` / `__torajs_rc_dec`** — `#[no_mangle] #[inline]
  pub unsafe extern "C" fn`. Non-atomic single-threaded refcount;
  NULL pass-through, `FLAG_STATIC_LITERAL` bypass, WeakRef-on-zero
  hook to `runtime_weakref.c`. Mirrors `runtime_str.c:244-272`
  byte-for-byte so the `ssa_lower` IR-side `call __torajs_rc_*` is
  unchanged.
- **Tag constants** (`TAG_STR=0` through `TAG_ARR_ITER=17`) and
  **flag constants** (`FLAG_STATIC_LITERAL=4`, `FLAG_FROZEN=16`,
  cycle-collector `COLOR_*`, …) — single source of truth shared
  across per-type crates so the C `#define` drift risk is removed.

## Quick start

```rust
use torajs_rc::{HeapHeader, TAG_STR, __torajs_rc_inc, __torajs_rc_dec};
use std::ffi::c_void;

// A torajs Str heap layout would look like:
//   [header: 8][len: 8][bytes: N]
// where `header` is exactly the `HeapHeader` from this crate.
#[repr(C, align(8))]
struct Str {
    header: HeapHeader,
    len: u64,
    // bytes follow at offset 16
}

let mut s = Str {
    header: HeapHeader { refcount: 1, type_tag: TAG_STR, flags: 0 },
    len: 5,
};

let p = &mut s as *mut Str as *mut c_void;
unsafe { __torajs_rc_inc(p); }              // refcount → 2
let freed = unsafe { __torajs_rc_dec(p); }; // refcount → 1, returns 0
let freed2 = unsafe { __torajs_rc_dec(p); };// refcount → 0, returns 1
assert_eq!(freed, 0);
assert_eq!(freed2, 1);
// caller now owns `s` exclusively and is responsible for the per-type drop.
```

## Why non-atomic

torajs runtime is **single-threaded** today (matches JS spec's single
event-loop model). Plain `u32` inc/dec is what the C code did; using
`AtomicU32` here would either compile to identical asm under `Relaxed`
ordering (no win) or inhibit LLVM auto-vectorize on the occasional
batched walk (real regression risk). When threading lands a new API
variant will be added — the current design pins the single-threaded
contract explicitly in the safety docs.

## Safety

All public functions are `unsafe`. Caller guarantees:

- `p` is null OR a valid `*mut HeapHeader` with an initialized header.
- No concurrent mutation (single-threaded contract).
- For `rc_dec`: when it returns `1`, caller owns the memory exclusively
  and runs the per-type drop + free. The WeakRef hook has already
  fired by the time `rc_dec` returns 1, so subsequent free is safe.

## Performance

3 criterion bench groups under `benches/rc.rs`:

- `inc_dec_pair` — the slot-copy + drop shape; every refcounted
  assignment in user code lowers to this.
- `inc_null_passthrough` — null-fast-path; should compile to a
  single `cmp + je`.
- `inc_static_literal_bypass` — flag bypass; should compile to
  two compares + branch.

Run with `cargo bench -p torajs-rc`.

Performance regression gate at `tests/perf_gate.rs`; see
[BUDGETS.md](BUDGETS.md).

## Where this is used

- [torajs] runtime — replaces the inline C `__torajs_rc_inc` /
  `__torajs_rc_dec` in `crates/torajs-runtime/src/runtime_str.c`.
  Linked into the final `tr` AOT binary; resolves the same
  `__torajs_rc_*` symbols `ssa_lower` emits IR-level extern calls
  to.

## License

Licensed under either of

- Apache License, Version 2.0
  ([LICENSE-APACHE](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license
  ([LICENSE-MIT](LICENSE-MIT) or
  <https://opensource.org/licenses/MIT>)

at your option.

[torajs]: https://github.com/goliajp/torajs
[`torajs-pool`]: https://crates.io/crates/torajs-pool
[`docs/architecture-rewrite.md`]: https://github.com/goliajp/torajs/blob/develop/docs/architecture-rewrite.md
