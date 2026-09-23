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

/// Arm a TypeError when `s` is nullish — NULL (JS `null`), the
/// undefined sentinel cell, or the Substr-shaped sentinel (string
/// index OOB read). bun/JSC word the two differently, and NULL can
/// now be attributed to `null`: chunk 4 flipped the last producers
/// that spelled `undefined` as NULL, so the guard no longer has to
/// report every nullish as undefined. Anything else passes through
/// untouched. The arr twin has worded it per shape since chunk B
/// ([`torajs_arr::null_guard`]).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_null_check(s: *const c_void) {
    if s.is_null() {
        unsafe {
            __torajs_throw_type_error(b"null is not an object\0".as_ptr() as *const _);
        }
    } else if crate::undef_sentinel::is_undef(s as *const u8)
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
