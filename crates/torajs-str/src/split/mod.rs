//! Str split — `s.split(sep)` materializes an `Array<Substring>`;
//! `SplitIter` is its zero-alloc iterator counterpart.
//!
//! ## Single-block layout
//!
//! `__torajs_str_split` allocates ONE heap block holding:
//!
//! ```text
//! [ARR header :24][N ptr slots :8N][N inline substr structs :32N]
//!  ^header at +0  ^ARR_DATA at +24 ^substrs_base at +24+8N
//! ```
//!
//! Each ptr slot stores the address of its corresponding inline
//! substr struct (so the array indexes look standard); each inline
//! substr carries [`FLAG_SUBSTR_INLINE`] so the per-substr drop is
//! a no-op aside from decrementing the parent Str's refcount. The
//! whole block frees in one `__torajs_arr_free` call once
//! refcount=0.
//!
//! Cuts the malloc count from `N+1` (one arr + N substrs) to
//! exactly 1, dominant win on tight loops like `s.split(',')` or
//! `.split('\n')`.
//!
//! ## Pool
//!
//! [`pool`] is a per-cap LIFO cache (16 slots). `arr_free`'s
//! `SPLIT_BLOCK` branch calls [`pool::free_push`] to recycle;
//! [`pool::alloc`] pops by exact-cap match for the next split.
//! Cap match is O(N)-with-tiny-N because tight loops typically
//! see one dominant cap value (LIFO head matches on first compare).
//!
//! Cross-TU dispatch (C-side `__torajs_arr_free` → Rust pool):
//! the C body forwards SPLIT_BLOCK blocks to
//! [`pool::__torajs_split_block_free_push`] which returns `true`
//! when accepted into the pool and `false` when the pool is full
//! (caller falls through to libc free).
//!
//! ## Iter
//!
//! [`ops::__torajs_split_iter_init`] / `_drop` initialize a
//! caller-provided 48-byte stack struct holding `(parent, sep,
//! pos, exhausted)`. `__torajs_split_iter_next` is still LLVM-IR
//! emitted in `ssa_inkwell::define_split_iter_next` (consolidates
//! into Rust in P3.1-g).

pub mod ops;
pub mod pool;
