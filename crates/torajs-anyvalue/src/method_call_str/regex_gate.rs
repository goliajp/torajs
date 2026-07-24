//! ES §22.1.3.14 step 2.b (matchAll) / §22.1.3.20 step 2.b
//! (replaceAll) — the shared "searchValue is a RegExp without the `g`
//! flag" gate, hoisted ahead of every operand coercion.
//!
//! Both kernels already reject a non-global RegExp, but only once
//! their operands have been coerced: `generic_str_this` runs
//! ToString(this) (step 3) before dispatching, and the REPLACE arm
//! runs ToString(replaceValue) (step 6.a) before calling the kernel.
//! Spec order puts step 2.b ahead of both, so a user `toString` on
//! the receiver or the replacement must NOT run when the search
//! argument already disqualifies the call (test262
//! `String/prototype/replaceAll/searchValue-flags-no-g-throws`, which
//! asserts the poisoned counter stays 0).
//!
//! A child module reaches its parent's private items, so
//! [`super::regexp_cell`] is used directly with no visibility churn.

use super::*;

unsafe extern "C" {
    /// torajs-regex — non-zero iff the cell carries the flag bit.
    fn __torajs_regex_has_flag(re: *const c_void, bit: i64) -> i64;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// `g` bit mirror (torajs-regex parser / `member_props_regexp.rs`).
const RE_FLAG_G: i64 = 0x02;

/// True when the pending TypeError was recorded — the caller must
/// abandon the dispatch without coercing anything. False for every
/// other method id, a non-RegExp search argument, and a RegExp that
/// carries `g` (all of which continue on their normal path).
///
/// # Safety
/// `argv` addresses `argc` valid AnyValue bit patterns.
pub(crate) unsafe fn reject_non_global_regex_search(mid: i64, argv: *const u64, argc: i64) -> bool {
    if mid != ANY_METHOD_REPLACE_ALL && mid != ANY_METHOD_MATCH_ALL {
        return false;
    }
    if argc < 1 {
        return false;
    }
    // SAFETY: `argc >= 1` was just checked; caller owns the buffer.
    let search = unsafe { *argv };
    // SAFETY: caller invariant on the AnyValue bit pattern.
    let Some(re) = (unsafe { regexp_cell(search) }) else {
        return false;
    };
    // SAFETY: `re` is a live RegExp cell from `regexp_cell`.
    if unsafe { __torajs_regex_has_flag(re, RE_FLAG_G) } != 0 {
        return false;
    }
    let msg = if mid == ANY_METHOD_REPLACE_ALL {
        c"String.prototype.replaceAll called with a non-global RegExp argument"
    } else {
        c"String.prototype.matchAll called with a non-global RegExp argument"
    };
    unsafe { __torajs_throw_type_error(msg.as_ptr()) };
    true
}
