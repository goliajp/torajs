//! `__torajs_bigint_drop` / `__torajs_bigint_drop_rc` — free a
//! BigInt heap block.
//!
//! Two entry points:
//! - [`__torajs_bigint_drop`] — direct free without rc check.
//!   Called by `runtime_str.c`'s `value_drop_heap` dispatch for
//!   the `TAG_BIGINT` arm AFTER `__torajs_rc_dec` returned 1
//!   (last owner). Don't call from binding-drop sites.
//! - [`__torajs_bigint_drop_rc`] — rc-aware drop. Decrements
//!   first; frees only on last owner. Used by ssa_lower's
//!   `emit_drop_value Type::BigInt` for bindings going out of
//!   scope.
//!
//! Both NULL-safe.

use core::ffi::c_void;

unsafe extern "C" {
    /// libc `free` — declared directly per the torajs-str pattern
    /// (avoids `libc` crate dep). Pairs with `runtime_bigint.c`'s
    /// historical `malloc` allocations (which this module is
    /// progressively replacing during the P3.3 port).
    fn free(p: *mut c_void);

    /// Cross-tier rc decrement — resolves against `libtorajs_rc.a`
    /// at link time. Returns 1 when the refcount hit zero (caller
    /// owns the now-dangling pointer and must free); 0 otherwise.
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
}

/// Direct free without rc check. NULL-safe.
///
/// # Safety
/// `p` must be either NULL or a valid pointer to a BigInt heap
/// block previously allocated via `malloc` (or its Rust-side
/// equivalent in a future sub-step). The caller asserts that
/// no other owner holds a reference — typically by having just
/// observed `__torajs_rc_dec` return 1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_drop(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    unsafe { free(p) };
}

/// rc-aware drop. Decrements; frees iff this was the last owner.
/// NULL-safe.
///
/// # Safety
/// `p` must be either NULL or a valid pointer to a BigInt heap
/// block with a live refcount header (offset 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_drop_rc(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    if unsafe { __torajs_rc_dec(p) } != 0 {
        unsafe { __torajs_bigint_drop(p) };
    }
}
