//! Str + Substr heap types + small-Str pool + per-op string fns
//! for the torajs AOT TypeScript runtime.
//!
//! Layer-2 substrate of the architecture rewrite (`docs/architecture-
//! rewrite.md` P3). Depends only on [`torajs-rc`] for the universal
//! heap header + `Tag::Str` + `FLAG_STATIC_LITERAL`. Provides
//! pointer-stable `Str` heap blocks plus the pool-aware alloc /
//! free path that the still-LLVM-IR-emitted `__torajs_str_alloc` /
//! `__torajs_str_drop` (in `ssa_inkwell`) and the remaining C-side
//! helpers in `crates/torajs-runtime/src/runtime_str.c` call into.
//!
//! ## Layout
//!
//! ```text
//! Str    = [header:8][len:8][bytes:N]              prefix 16
//! Substr = [header:8][len:8][parent_ptr:8][off:8]  prefix 32
//! ```
//!
//! Sub-step matrix:
//! | Phase    | Adds                                                       |
//! |----------|------------------------------------------------------------|
//! | P3.1-a   | Str layout constants + small-Str LIFO pool + alloc / free  |
//! | P3.1-b   | Substr layout + Substr pool + split-block pool             |
//! | P3.1-c   | Str eq / to_number / concat                                |
//! | P3.1-d   | Lookup ops (slice / index_of / includes / starts_with /…)  |
//! | P3.1-e   | Transform ops (case / trim / pad / repeat / replace / …)   |
//! | P3.1-f   | Split + SplitIter                                          |
//! | P3.1-g   | print / print_err + ssa_inkwell IR-side defines ported     |
//!
//! ## Design — Rust idiomatic, not a C transcription
//!
//! Per the project's pure-Rust + 自研 pillars (`docs/design-
//! principles.md`), the API is Rust-first:
//!
//! - [`alloc::StrBlock`] is a `NonNull<u8>` newtype with safe
//!   accessor methods (`len()` / `as_bytes()` / `as_bytes_mut()` /
//!   `header()` / `header_mut()`). The whole concept "owned Str
//!   block" lives in the type, not in implicit pointer math
//!   scattered across call sites.
//! - [`pool::SmallStrPool`] exposes `pop()` / `push()` methods over
//!   a global static (32 slots × 24-byte blocks). Slots are stored
//!   in `AtomicPtr` / `AtomicUsize` for Rust's safety story; the
//!   runtime is single-threaded today so `Ordering::Relaxed`
//!   compiles to identical asm vs raw `static mut`.
//! - The `extern "C"` wrappers ([`alloc::__torajs_str_alloc_pooled`]
//!   / [`alloc::__torajs_str_free`]) are thin (≤ 10 lines each) —
//!   null check + transmute + delegate to the idiomatic core.
//!
//! ## ABI invariants (must not change)
//!
//! - `STR_HDR_SIZE` = 16, `STR_LEN_OFF` = 8, `STR_DATA_OFF` = 16.
//!   `ssa_inkwell` emits const-offset GEPs at every Str access
//!   site; the runtime_str.c macros (`__TORAJS_STR_LEN(p)`,
//!   `__TORAJS_STR_DATA(p)`, etc.) mirror these offsets. Drift
//!   silently corrupts every Str load / store in the runtime.
//! - [`layout::packed_header_init`] = `1 | (Tag::Str as u64) << 32`
//!   — a single 8-byte store sets `refcount=1, type_tag=0 (Str),
//!   flags=0` at alloc time.
//! - `STR_POOL_PAYLOAD` = 16, `STR_POOL_SLOTS` = 32. Same fixed
//!   block size class as the pre-rewrite C pool, so a Str block
//!   freed by `__torajs_str_free` and re-popped by
//!   `__torajs_str_alloc_pooled` round-trips bit-identical.
//!
//! ## Why `std`, not `no_std`
//!
//! Same reason as [`torajs-rc`] / [`torajs-anyvalue`] / [`torajs-
//! throw`]: cargo's `cargo test` + dual `crate-type = ["rlib",
//! "staticlib"]` + `no_std` combination triggers a precompiled-
//! core panic-strategy mismatch (the test runner forces unwind
//! panics, precompiled core demands abort) that has no clean fix
//! on stable. `std` staticlibs link cleanly at `tr build` time
//! (cc + LLVM-LTO tolerates std symbol overlap between Rust-
//! emitted .a's).

pub mod alloc;
pub mod concat;
pub mod eq;
pub mod json;
pub mod json_parse;
pub mod layout;
pub mod literals;
pub mod lookup;
pub mod pool;
pub mod print;
pub mod slice;
pub mod split;
pub mod substr;
pub mod substr_methods;
pub mod symbol;
pub mod to_number;
pub mod transform;

// Re-export the small surface the rest of the workspace (and the
// FFI consumers) reach for most often. Keeping this list tight
// pins the public crate API; full surface is still reachable via
// the module paths above.
pub use alloc::{
    __torajs_str_alloc, __torajs_str_alloc_pooled, __torajs_str_drop, __torajs_str_free, StrBlock,
};
pub use concat::__torajs_str_concat;
pub use eq::{__torajs_str_eq, __torajs_str_eq_cstr};
pub use json::__torajs_json_quote_str;
pub use json_parse::{
    __torajs_json_arr_first, __torajs_json_arr_step, __torajs_json_eat_char,
    __torajs_json_parse_bool, __torajs_json_parse_float, __torajs_json_parse_int,
    __torajs_json_parse_string,
};
pub use layout::{
    STR_DATA_OFF, STR_HDR_SIZE, STR_LEN_OFF, STR_POOL_PAYLOAD, STR_POOL_SLOTS, block_size,
    packed_header_init,
};
pub use literals::{__torajs_null_to_str, __torajs_undefined_to_str};
pub use lookup::{
    __torajs_str_char_code_at, __torajs_str_ends_with, __torajs_str_ends_with_from,
    __torajs_str_includes, __torajs_str_includes_from, __torajs_str_index_of,
    __torajs_str_index_of_from, __torajs_str_last_index_of, __torajs_str_last_index_of_from,
    __torajs_str_locale_compare, __torajs_str_starts_with, __torajs_str_starts_with_from,
};
pub use print::{__torajs_str_print, __torajs_str_print_err, __torajs_substr_print};
pub use slice::__torajs_str_slice;
pub use split::ops::{
    __torajs_split_iter_drop, __torajs_split_iter_init, __torajs_str_split, SplitIter,
};
pub use split::pool::__torajs_split_block_free_push;
pub use substr::{
    __torajs_substr_create, __torajs_substr_drop, FLAG_SUBSTR_INLINE, SUBSTR_LEN_OFF,
    SUBSTR_OFFSET_OFF, SUBSTR_PARENT_OFF, SUBSTR_SIZE, SubstrBlock,
};
pub use to_number::{__torajs_str_to_number, parse_number};
pub use transform::case::{__torajs_str_to_lower, __torajs_str_to_upper};
pub use transform::construct::{
    __torajs_str_at, __torajs_str_char_at, __torajs_str_from_char_code, __torajs_str_repeat,
    __torajs_str_substr, __torajs_str_substring,
};
pub use transform::pad::{__torajs_str_pad_end, __torajs_str_pad_start};
pub use transform::replace::{__torajs_str_replace, __torajs_str_replace_all};
pub use transform::trim::{__torajs_str_trim, __torajs_str_trim_end, __torajs_str_trim_start};

// torajs-rc's `__torajs_rc_dec` calls into a WeakRef hook whenever
// a block reaches refcount = 0. At `tr build` link time, the
// runtime_weakref.c TU provides the symbol; in the cargo-test
// binary there is no such TU. One global stub here covers every
// submodule's `cargo test` run — defining it per-module collides
// at link time. Mirrors the per-test stub in `torajs-rc`'s own
// test module.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_target_dying(_target: *mut std::ffi::c_void) {}
