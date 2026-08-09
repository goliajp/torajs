//! RFC 20260810-arr-sparse-grow 刀 D — the sparse-tail input gate
//! shared by the four combinator kernels (`all` / `race` / `any` /
//! `allSettled`). Sibling of `combinator.rs` (500-line file cap).

use core::ffi::c_void;

unsafe extern "C" {
    /// torajs-throw — catchable RangeError for the sparse-tail gate.
    fn __torajs_throw_range_error(msg: *const u8);
}

/// `torajs_rc::FLAG_ARR_SPARSE_TAIL` mirror (bit 6 of the u16 header
/// flags at +6) — this crate mirrors array layout facts rather than
/// depending on torajs-rc.
const FLAG_ARR_SPARSE_TAIL: u16 = 1 << 6;

/// The combinator walks read raw slots over `[0, len)`; a sparse
/// tail has no storage behind `[extent, len)`. Loud reject (with the
/// RangeError already recorded) until the combinators grow real
/// sparse support; the caller answers its rejected-promise neutral.
pub(crate) unsafe fn sparse_input_rejects(arr: *mut c_void) -> bool {
    let flags = unsafe { *((arr as *const u8).add(6) as *const u16) };
    if flags & FLAG_ARR_SPARSE_TAIL == 0 {
        return false;
    }
    unsafe {
        __torajs_throw_range_error(
            b"sparse array tail is not yet supported in a Promise combinator\0".as_ptr(),
        )
    };
    true
}
