//! BigInt signed compare + equality.
//!
//! Port of `runtime_bigint.c` lines 294-308 (P3.3-f, 2026-05-23).
//!
//! Two extern fns:
//! - [`__torajs_bigint_cmp`] — signed compare → -1 / 0 / 1
//! - [`__torajs_bigint_eq`] — `cmp == 0` shortcut
//!
//! Sign handling:
//! - different signs: negative side is smaller (with the signed-zero
//!   special case where both are zero → 0)
//! - same signs: compare magnitudes; for negative operands, flip the
//!   magnitude-cmp result (larger magnitude = smaller signed value)
//!
//! Reuses `crate::arith::mag_cmp` (already `pub(crate)`).

use core::ffi::c_void;

use crate::arith::mag_cmp;
use crate::internal::{read_len, read_sign};

/// `cmp(a, b)` for BigInt — returns -1 / 0 / 1.
///
/// # Safety
/// `a_` and `b_` must be valid BigInt heap pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_cmp(a_: *const c_void, b_: *const c_void) -> i64 {
    let a = a_ as *const u8;
    let b = b_ as *const u8;
    unsafe {
        let a_sign = read_sign(a);
        let b_sign = read_sign(b);
        if a_sign != b_sign {
            // Both zero (one stored as positive, one as negative by some
            // upstream slip) is canonically equal. BigInt invariant says
            // zero is always positive so the second branch shouldn't fire
            // in practice — defensive.
            if read_len(a) == 0 && read_len(b) == 0 {
                return 0;
            }
            return if a_sign != 0 { -1 } else { 1 };
        }
        let m = mag_cmp(a, b) as i64;
        if a_sign != 0 { -m } else { m }
    }
}

/// `eq(a, b)` for BigInt — returns 1 if equal, 0 otherwise.
///
/// # Safety
/// Same as [`__torajs_bigint_cmp`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_eq(a_: *const c_void, b_: *const c_void) -> i64 {
    if unsafe { __torajs_bigint_cmp(a_, b_) } == 0 {
        1
    } else {
        0
    }
}
