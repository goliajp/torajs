//! RFC 20260707-undefined-sentinel-repr chunk 1 — null-receiver
//! guard for un-narrowed nullish Str consumption. A missed
//! exec/match capture slot (`m[1]` on `/a(b)?/.exec("a")`) is a
//! NULL Str per the 591 convention (NULL = JS undefined); a deref
//! of it through the inline `.length` load must be a catchable
//! TypeError per §13.3.2.1, not a SIGSEGV.
//!
//! ssa_lower emits `str_null_check(s)` + `emit_throw_check(None)`
//! in front of the load when the receiver is a nullable-str
//! source (index-load off a nullable-arr binding, or a binding
//! initialized from one). Mirrors torajs-arr/null_guard.rs.

use core::ffi::c_void;

unsafe extern "C" {
    /// Cross-tier — torajs-throw. Arms a catchable TypeError; the
    /// caller's SSA-level emit_throw_check propagates it.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Arm a TypeError when `s` is nullish — NULL, the undefined
/// sentinel cell, or the Substr-shaped sentinel (string index OOB
/// read; bun/JSC wording; all report the undefined wording until
/// the remaining NULL-means-undefined producers flip and NULL can
/// be attributed to JS null, RFC 20260707 chunk 4). Anything else
/// passes through untouched.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_null_check(s: *const c_void) {
    if s.is_null()
        || crate::undef_sentinel::is_undef(s as *const u8)
        || crate::undef_sentinel::is_substr_undef(s as *const u8)
    {
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
        unsafe { __torajs_str_null_check(&x as *const u64 as *const c_void) };
    }
}
