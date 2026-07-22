//! RFC 20260722-find-miss-undefined-sentinel chunk B — nullish-
//! receiver guard for pointer-shaped heap slots (Obj / Arr /
//! Closure). A `find`/`findLast` miss (and the C2b optional-field
//! producer) answers the generic immortal undefined cell
//! (`undef_cell.rs`); a member read through it must be a catchable
//! TypeError per §13.3.2.1, not a deref past the bare 8-byte static
//! header. NULL keeps meaning JS `null` (591 convention) with its
//! own wording. Mirrors torajs-str/null_guard.rs and
//! torajs-arr/null_guard.rs: the throw helper RETURNS (pending-throw
//! TLS), ssa_lower's `emit_throw_check` right after diverts before
//! the deref executes.

use core::ffi::c_void;

unsafe extern "C" {
    /// Cross-tier — torajs-throw. Arms a catchable TypeError; the
    /// caller's SSA-level emit_throw_check propagates it. Signature
    /// matches the existing freeze.rs declaration.
    fn __torajs_throw_type_error(msg: *const u8);
}

/// Arm a TypeError when `p` is nullish — NULL (JS null) or the
/// generic undefined sentinel cell (bun/JSC wording per shape).
/// Any live heap cell passes through untouched.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_heap_nullish_check(p: *const c_void) {
    if p.is_null() {
        unsafe {
            __torajs_throw_type_error(b"null is not an object\0".as_ptr());
        }
    } else if crate::undef_cell::is_undef_cell(p as *const u8) {
        unsafe {
            __torajs_throw_type_error(b"undefined is not an object\0".as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_cell_passes_through() {
        let x: u64 = 0;
        // Any non-nullish pointer is untouched (no read, no throw).
        unsafe { __torajs_heap_nullish_check(&x as *const u64 as *const c_void) };
    }
}
