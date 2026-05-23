//! Array<T> + Array<Any> substrate for the torajs AOT TypeScript
//! runtime.
//!
//! Layer-3 substrate of the architecture rewrite (`docs/architecture-
//! rewrite.md` P4.1). Heap-allocated dynamic array with a refcounted
//! universal heap header + `len` + `cap` + `slots[]`. Two sub-flavors
//! (selected by `type_tag` + `FLAG_ARR_ANY`):
//!
//! - `Array<T>` — slots are 8-byte raw values (i64 / f64 / Str ptr / ...)
//! - `Array<Any>` — slots are 16-byte tag/value pairs (boxed-Any)
//!
//! Pool-aware free — small-cap blocks (`cap ≤ ARR_POOL_PAYLOAD`) return
//! to a thread-local LIFO pool; large blocks go straight to libc free.
//! The pool itself lives in C (`runtime_str.c::arr_pool_*`) for now —
//! P4.1+ ships ports of each public fn over time.
//!
//! ## Sub-step matrix (P4.1)
//!
//! | Phase   | Adds                                                |
//! |---------|-----------------------------------------------------|
//! | P4.1-a  | scaffold + ArrHeader layout + `__torajs_arr_drop`   |
//! | P4.1-b  | basic ops: push / pop / get / set / len / alloc     |
//! | P4.1-c  | iter (forEach/map/filter/reduce + ArrIter struct)   |
//! | P4.1-d  | slice / concat / join / sort / reverse              |
//! | ...     | (continued — Array surface is large; one family / step) |
//!
//! ## Why `std`, not `no_std`
//!
//! Same reason as torajs-rc / torajs-str / torajs-num / torajs-bigint:
//! cargo's `cargo test` + dual `crate-type = ["rlib", "staticlib"]`
//! + `no_std` combination triggers a precompiled-core panic-strategy
//! mismatch with no clean fix on stable. `std` staticlibs link cleanly
//! at `tr build` time.

pub mod drop;
pub mod layout;

pub use drop::__torajs_arr_drop;
