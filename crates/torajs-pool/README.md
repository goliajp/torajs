# torajs-pool

[![Crates.io](https://img.shields.io/crates/v/torajs-pool?style=flat-square&logo=rust)](https://crates.io/crates/torajs-pool)
[![docs.rs](https://img.shields.io/docsrs/torajs-pool?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-pool)
[![License](https://img.shields.io/crates/l/torajs-pool?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-pool?style=flat-square)](https://crates.io/crates/torajs-pool)

Bounded fixed-size memory pool — thread-local LIFO free-list for
fixed-size struct types. `no_std + alloc`. Single dependency: `alloc`.

Extracted from the [torajs] AOT TypeScript runtime where it shipped
as P-PERF.A6 (commit `8f754ca`, 2026-05-22) with measured wins:

- **promise-await-100k: −41 %** wall-clock (2.99 → 1.75 ms)
- **async-fn-call-100k: −36 %** (2.96 → 1.90 ms)
- **promise-then-100k: −32 %** (5.75 → 3.88 ms)
- **integrated geomean vs bun-aot: 4.16× → 4.41×** (+6.1 %)

…on the same 5-pass-median bench protocol versus the previous
straight-`malloc` / `free` path. See the [torajs] PERFORMANCE.md
for the full bench table + reproduction protocol.

## Quick start

```rust
use torajs_pool::FixedPool;

#[repr(C)]
struct Promise {
    state: u8,
    _pad: [u8; 7],
    value: i64,
    // Reuses this slot as the "next" link while parked in the pool.
    callbacks: *mut u8,
}

// `next` field is at byte offset 16 in this layout (state+pad+value).
let pool: FixedPool<Promise, 32> = unsafe {
    FixedPool::new_with_next_offset(16)
};

unsafe {
    let p = pool.acquire();
    (*p).state = 1;
    (*p).value = 42;
    (*p).callbacks = std::ptr::null_mut();
    // ... use p ...
    pool.release(p);
}
```

## API

| Item | Description |
|---|---|
| `FixedPool<T, const CAP: usize>` | The pool type. `CAP` is the bound (slots past `CAP` get `dealloc`ed on `release`). |
| `unsafe fn new_with_next_offset(offset: usize) -> Self` | Build an empty pool. `offset` is the byte offset of the pointer-sized "next" link within `T`. |
| `unsafe fn acquire(&self) -> *mut T` | Pop the LIFO head (hot) or fresh-alloc (cold). |
| `unsafe fn release(&self, p: *mut T)` | Push to the LIFO head (hot, count < CAP) or `dealloc` (overflow). |
| `fn pooled(&self) -> usize` | Current parked slot count. |
| `fn capacity(&self) -> usize` | Compile-time `CAP`. |
| `impl Drop` | Frees every parked entry. |

## Safety

This is a low-level pool primitive. The API is `unsafe` end-to-end:

- `acquire` returns uninitialized memory other than the pool's "next"
  link bytes. The caller initializes all other fields before user code
  reads them.
- `release` does NOT call `Drop::drop`. The caller tears down `T`'s
  fields before releasing.
- Single-threaded by construction (`FixedPool: !Sync`). Wrap in
  `Mutex` if you need multi-thread access.
- Layout invariant: `next_offset .. next_offset + 8` must be within
  `T` and aligned for `*mut u8`. Pass the right offset to
  `new_with_next_offset`.

## Why bounded

A long-running daemon that builds + drops fixed-size structs
forever would otherwise grow the pool unbounded and pin memory.
The `CAP` bound trades worst-case memory for amortized fast-path
allocation. Tunable per call site via the const-generic parameter.

## Why LIFO + reuse-the-next-field

LIFO (vs FIFO queue) pops a slot whose memory was most recently
touched — likely still in cache. Reusing one of `T`'s pointer
fields as the link avoids a side-table of bookkeeping pointers;
the pool's per-entry overhead is zero bytes.

## Benchmarks

Three criterion bench groups under `benches/pool.rs`:

- `acquire_release_hot` — steady-state acquire-release on warm pool;
  the dominant case in torajs Promise traffic.
- `acquire_cold_malloc_baseline` — pool empty each iter; isolates the
  malloc/free cost the pool is replacing.
- `release_overflow_bound` — release past `CAP`; ensures the bound
  check stays fast.

Run with `cargo bench -p torajs-pool`.

Performance regression gate at `tests/perf_gate.rs`; see
[BUDGETS.md](BUDGETS.md) for the per-path budget table.

## Where this is used

- [torajs] runtime, `runtime_promise.c` (current C-implementation
  ship line, see `git log -- crates/torajs-runtime/src/runtime_promise.c
  | grep 8f754ca` for the bench-anchor commit). The C side will be
  rewritten to Rust + linked against this crate as the rewrite
  progresses (per `docs/architecture-rewrite.md`).

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
