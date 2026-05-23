//! Map key equality — SameValueZero (ES spec §7.2.10).
//!
//! Port of `runtime_map.c::map_keys_equal` (P4.3-b, 2026-05-23).
//! Used by [`crate::probe::map_lookup_slot`] (P4.3-c) + the get/has/
//! set/delete extern fns (P4.3-d..-f).
//!
//! Equality rules:
//! - `null === null`, `undefined === undefined`.
//! - `bool` / `i64`: bitwise eq of payload.
//! - `f64`: `NaN === NaN` (both NaN → equal), `+0 === -0` (IEEE eq).
//! - `heap`:
//!   - Pointer-identity short-circuit (interned Str literals + same-
//!     object cases). NULL handled.
//!   - Different type_tag → unequal.
//!   - Both `Str` → byte-by-byte compare via cross-tier
//!     `__torajs_str_eq`.
//!   - Other heap types → pointer identity only (already short-
//!     circuited above; returns false here).

use core::ffi::c_void;

use crate::layout::{
    ANY_BOOL, ANY_F64, ANY_HEAP, ANY_I64, ANY_NULL, ANY_UNDEF, HeapHeader, TAG_STR,
};

unsafe extern "C" {
    /// Cross-tier — torajs-str's content equality. Returns 1 iff
    /// `a` + `b` are both live Str blocks with identical bytes.
    fn __torajs_str_eq(a: *const u8, b: *const u8) -> i64;
}

/// SameValueZero comparison between two Any-tagged keys.
///
/// # Safety
/// For `ANY_HEAP` tag, payloads are either NULL or valid live heap
/// pointers with universal headers (type_tag read at offset 4).
pub(crate) unsafe fn map_keys_equal(ta: u8, pa: u64, tb: u8, pb: u64) -> bool {
    if ta != tb {
        return false;
    }
    match ta {
        ANY_NULL | ANY_UNDEF => true,
        ANY_BOOL | ANY_I64 => pa == pb,
        ANY_F64 => {
            let da = f64::from_bits(pa);
            let db = f64::from_bits(pb);
            if da.is_nan() {
                db.is_nan()
            } else if db.is_nan() {
                false
            } else {
                // IEEE eq: +0 == -0 holds here.
                da == db
            }
        }
        ANY_HEAP => {
            let pa_p = pa as *mut c_void;
            let pb_p = pb as *mut c_void;
            if pa_p == pb_p {
                return true;
            }
            if pa_p.is_null() || pb_p.is_null() {
                return false;
            }
            let ha = pa_p as *const HeapHeader;
            let hb = pb_p as *const HeapHeader;
            let ta = unsafe { (*ha).type_tag };
            let tb = unsafe { (*hb).type_tag };
            if ta != tb {
                return false;
            }
            if ta == TAG_STR {
                unsafe { __torajs_str_eq(pa_p as *const u8, pb_p as *const u8) != 0 }
            } else {
                // Non-Str heap: identity already checked above.
                false
            }
        }
        _ => pa == pb,
    }
}
