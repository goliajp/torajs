//! RFC 20260810-arr-sparse-grow 刀 D — the loud gate in front of
//! every `Array<Any>` kernel that walks `[0, len)` (raw slot reads,
//! bulk rc ranges, or per-index funnel loops). Under a sparse tail
//! `len` can be up to 2^32-1 while only `extent` slots exist: the
//! raw walkers would read past the buffer, and even the
//! funnel-riding loops would spin ~4e9 rounds / allocate a 32GB
//! result — a hang presenting as a timeout. Each gated op converts
//! to real sparse support as test262 cases demand it (independent
//! follow-up knives); until then the reject is a catchable
//! RangeError naming the operation.

use core::ffi::c_void;

use torajs_rc::{FLAG_ARR_SPARSE_TAIL, HeapHeader};

unsafe extern "C" {
    /// Cross-tier — torajs-throw. Records a catchable RangeError;
    /// the caller's SSA-level emit_throw_check propagates it.
    fn __torajs_throw_range_error(msg: *const u8);
}

/// True (with the RangeError already recorded) when `arr` carries a
/// sparse tail — the caller returns its neutral value immediately.
/// NULL and non-sparse cells answer false with a single flags read.
///
/// # Safety
/// `arr` is NULL or a valid heap cell pointer.
#[inline]
pub(crate) unsafe fn sparse_tail_rejects(arr: *const c_void, msg: *const u8) -> bool {
    if arr.is_null() {
        return false;
    }
    if unsafe { (*(arr as *const HeapHeader)).flags } & FLAG_ARR_SPARSE_TAIL == 0 {
        return false;
    }
    unsafe { __torajs_throw_range_error(msg) };
    true
}
