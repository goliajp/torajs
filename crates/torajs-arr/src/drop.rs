//! `__torajs_arr_drop` — rc-aware drop for Array heap blocks.
//!
//! Port of `ssa_inkwell::define_arr_drop` (IR-emitted before P4.1-a;
//! now a Rust extern with identical semantics).
//!
//! Semantics (mirror IR shape 1:1):
//! 1. NULL → no-op
//! 2. Flags has `FLAG_STATIC_LITERAL` → no-op (`.rodata`-baked arrays
//!    don't have a refcount we own)
//! 3. `__torajs_rc_dec(p)` → if last owner (returned 1):
//!    a. `__torajs_arrprops_drop_entry(p)` — release any associated
//!       key-value props (no-op for arrays that never had `arr.x = v`)
//!    b. `__torajs_arr_free(p)` — pool-aware free (LIFO pool for
//!       cap ≤ ARR_POOL_PAYLOAD, libc free otherwise)
//!
//! Caller (ssa_lower `emit_drop_value Type::Arr`) is responsible for
//! walking refcounted ELEMENT types FIRST (e.g. `Arr<Str>` walks each
//! Str's rc_dec before calling here). This fn only owns the array
//! header + the slots' backing storage.

use core::ffi::c_void;

use torajs_rc::{FLAG_STATIC_LITERAL, HeapHeader};

unsafe extern "C" {
    /// Cross-tier — torajs-rc. Decrements rc; returns 1 if hit zero
    /// (caller takes ownership of the now-dangling pointer).
    fn __torajs_rc_dec(p: *mut c_void) -> i32;

    /// Cross-tier — runtime_str.c's pool-aware array free. Returns
    /// blocks with `cap ≤ ARR_POOL_PAYLOAD` to a LIFO pool; libc free
    /// for the rest.
    fn __torajs_arr_free(p: *mut c_void);

    /// Cross-tier — runtime_str.c's array-prop side-table. Drops the
    /// per-array key-value entry if one exists. No-op for the common
    /// case (most arrays never had `arr.x = v` written).
    fn __torajs_arrprops_drop_entry(p: *mut c_void);
}

/// rc-aware drop. NULL-safe + `FLAG_STATIC_LITERAL`-safe.
///
/// # Safety
/// `p` is either NULL or a valid Array heap block pointer with a live
/// universal heap header at offset 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_drop(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    // STATIC_LITERAL flag check — `.rodata` array literals must never
    // be rc-decremented (the store would page-fault) or freed.
    let header = unsafe { &*(p as *const HeapHeader) };
    if header.flags & FLAG_STATIC_LITERAL != 0 {
        return;
    }
    if unsafe { __torajs_rc_dec(p) } != 0 {
        unsafe {
            __torajs_arrprops_drop_entry(p);
            __torajs_arr_free(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_null_is_noop() {
        unsafe { __torajs_arr_drop(core::ptr::null_mut()) };
    }
}
