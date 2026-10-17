//! ES §22.1.3.14 steps 2.a-b (matchAll) / §22.1.3.20 steps 2.a-b
//! (replaceAll) — the shared "searchValue is a RegExp (IsRegExp)
//! without the `g` flag" gate, hoisted ahead of every operand coercion.
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

use super::*;

/// True when the pending TypeError was recorded — the caller must
/// abandon the dispatch without coercing anything. False for every
/// other method id, a primitive search argument, an Object IsRegExp
/// says no for, and one whose `flags` carry `g` (all of which continue
/// on their normal path).
///
/// # Safety
/// `argv` addresses `argc` valid AnyValue bit patterns.
#[inline(never)]
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
    if !unsafe { crate::iter_zip_shared::av_is_object(search) } {
        return false;
    }
    // Step 2.a's IsRegExp and 2.b's `flags` read, for any Object: the
    // protocol seam's, behind which the reads, the flag bits and the
    // TypeError live with the rest of the RegExp world. Outside that
    // world no Object answers IsRegExp yes, and the seam's stub
    // answers 0. It answers nonzero when the call is abandoned.
    unsafe {
        crate::dispatch_seam::__torajs_regexp_protocol_call(
            search,
            torajs_rc::ANY_METHOD_GET_FLAGS,
            core::ptr::null(),
            mid,
        ) != 0
    }
}
