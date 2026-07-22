//! RC-4 F1a — null-receiver guard for un-narrowed
//! `Nullable<Array<T>>` consumption. `re.exec(s)` / `s.match(re)`
//! answer null on miss (spec §22.2.6.2 / §22.1.3.13); a deref of
//! that null must be a catchable TypeError per §13.3.2.1, not a
//! SIGSEGV through the inline `.length` / element load.
//!
//! ssa_lower emits `arr_null_check(arr)` + `emit_throw_check(None)`
//! in front of the load when the receiver is a nullable-arr binding
//! (let-init exec/match shape) or a direct exec/match call chain.
//! Mirrors the `__torajs_obj_check_not_frozen` guard shape: the
//! throw helper RETURNS (pending-throw TLS), the throw-check right
//! after diverts before the deref executes.

use core::ffi::c_void;

unsafe extern "C" {
    /// Cross-tier — torajs-throw. Arms a catchable TypeError; the
    /// caller's SSA-level emit_throw_check propagates it. Signature
    /// matches the existing index_any.rs declaration (c_char).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Arm a TypeError when `p` is nullish — NULL, or the generic
/// undefined sentinel cell (RFC 20260722-find-miss chunk B: an
/// `Arr[]` `find`/`findLast` miss answers the cell, so a `.length`
/// / element load through it must throw like bun instead of
/// dereferencing past the bare static header; bun/JSC wording per
/// shape). Any live cell passes through untouched.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_null_check(p: *const c_void) {
    if p.is_null() {
        unsafe {
            __torajs_throw_type_error(b"null is not an object\0".as_ptr() as *const _);
        }
    } else if torajs_rc::undef_cell::is_undef_cell(p as *const u8) {
        unsafe {
            __torajs_throw_type_error(b"undefined is not an object\0".as_ptr() as *const _);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_null_passes_through() {
        let x: u64 = 0;
        // Any non-null pointer is untouched (no read, no throw).
        unsafe { __torajs_arr_null_check(&x as *const u64 as *const c_void) };
    }
}
