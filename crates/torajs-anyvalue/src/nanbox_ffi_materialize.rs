//! ShortStr → Heap+Str materialize helpers — Step 8b-C carve-out
//! from [`nanbox_ffi`] to keep that file's prod-only LOC ≤ 500.
//!
//! `nanbox_ffi.rs` houses the 8 `__torajs_anyv_*` shims; this
//! sibling holds the three small helpers those shims call:
//!
//! - [`materialize_short_str`] — widen a ShortStr AnyValue into a
//!   freshly-owned `*mut u8` Heap+Str pointer (refcount = 1).
//! - [`materialize_if_short`] — conditionally widen, returning an
//!   optional drop handle so the caller can keep its temporaries
//!   in scope.
//! - [`drop_materialized_str`] — symmetric drop for temporaries
//!   from `materialize_if_short`.
//!
//! Future polish (8d / 8e): inline byte parsing in `to_number` and
//! inline byte compare in `strict_eq` would eliminate the alloc
//! entirely on those hot paths. This file's surface stays narrow
//! enough that those rewrites are a clean drop-in.

use std::ffi::c_void;

use torajs_rc::__torajs_rc_dec;

use crate::nanbox::{AnyValue, is_short_str, short_str_bytes, short_str_len};

unsafe extern "C" {
    /// Heap+Str alloc + bytes-copy. Mirrors
    /// `torajs-str::__torajs_str_alloc`.
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// Per-type heap drop walker. Used to drop materialized temps
    /// once their rc hits zero.
    fn __torajs_value_drop_heap(child: *mut c_void);
}

/// Materialize a ShortStr-encoded AnyValue into a freshly-owned
/// `*mut u8` Heap+Str pointer (refcount = 1) whose bytes match
/// the ShortStr payload.
///
/// Used by shim paths that need the heap shape:
///
/// - `__torajs_anyv_to_str` returns this (caller expects a
///   freshly-owned Str pointer per the ToString contract).
/// - `__torajs_anyv_strict_eq` cross-type comparison
///   (ShortStr === Heap+Str).
/// - The compare / arith / add bridge that still consumes
///   `(Heap, ptr)` legacy pairs.
///
/// # Safety
///
/// Caller asserts [`is_short_str(v)`].
#[inline]
pub(crate) unsafe fn materialize_short_str(v: AnyValue) -> *mut u8 {
    debug_assert!(is_short_str(v));
    let len = short_str_len(v) as i64;
    let bytes = short_str_bytes(v);
    // SAFETY: bytes is a stack-resident [u8; 5]; `__torajs_str_alloc`
    // reads `len` bytes (len ≤ 5 per ShortStr invariant). Returns a
    // refcount=1 heap Str the caller owns.
    unsafe { __torajs_str_alloc(bytes.as_ptr(), len) }
}

/// If `v` is a ShortStr, materialize to a fresh Heap+Str pointer
/// and return `(cell_av, Some(ptr))`. Otherwise return `(v, None)`.
///
/// Callers feed `cell_av` into the legacy `(tag, value)` decoder
/// (`decode_to_tag_value`) and drop the returned `*mut u8` via
/// [`drop_materialized_str`] once the inner helper has run.
#[inline]
pub(crate) unsafe fn materialize_if_short(v: AnyValue) -> (AnyValue, Option<*mut u8>) {
    if is_short_str(v) {
        // SAFETY: is_short_str asserted; materialize gives a fresh
        // refcount=1 Heap+Str the caller owns until they drop.
        let p = unsafe { materialize_short_str(v) };
        (p as u64, Some(p))
    } else {
        (v, None)
    }
}

/// Drop a temporary Heap+Str obtained from [`materialize_if_short`].
/// Matches the rc_dec → value_drop_heap pattern used elsewhere in
/// the shim suite.
#[inline]
pub(crate) unsafe fn drop_materialized_str(p: *mut u8) {
    // SAFETY: p is a Heap+Str the caller exclusively owns.
    unsafe {
        let dec = __torajs_rc_dec(p as *mut c_void);
        if dec != 0 {
            __torajs_value_drop_heap(p as *mut c_void);
        }
    }
}
