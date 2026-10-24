//! The string lane of `__torajs_any_iter_next` (`iter_any`) —
//! §22.1.5.1, one code point per step. A sibling so the dispatch
//! module keeps to its cap; the lane order and the `iter_slot` /
//! `idx_slot` contract are the parent's.

use crate::iter_any_await::AwaitMode;
use core::ffi::c_void;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr};
use crate::nanbox_encode::__torajs_anyv_box_pointer;

unsafe extern "C" {
    /// torajs-str — the §22.1.5.1 step: the code point at code-unit
    /// index `i` as a fresh Str (rc 1), its unit span in `*adv`;
    /// NULL past the end.
    fn __torajs_str_code_point_str_at(s: *const u8, i: i64, adv: *mut i64) -> *mut u8;
    /// torajs-str — an owned plain Str (+1) off a Str or Substr view.
    fn __torajs_str_any_to_owned(s: *const u8) -> *mut u8;
}

/// The string lane of [`iter_next_inner`] — §22.1.5.1, one code
/// point per step. First step: flatten the receiver into `iter_slot`
/// (a ShortStr materializes, a Str is retained, a Substr view or a
/// String wrapper flattens to a plain Str), an owned reference the
/// caller releases with the slot at loop exit. Every step reads that
/// Str: `*idx_slot` is the code-unit cursor, the step's Str is fresh
/// (rc 1, handed to `*out`), and the cursor advances by the units the
/// code point spans.
pub(crate) unsafe fn string_step(
    recv: AnyValue,
    idx_slot: *mut i64,
    iter_slot: *mut AnyValue,
    out: *mut AnyValue,
    mode: AwaitMode,
) -> i64 {
    unsafe {
        if *iter_slot == VALUE_UNDEFINED {
            let s = crate::nanbox_ffi::__torajs_anyv_to_str(recv);
            let flat = __torajs_str_any_to_owned(s.cast::<u8>());
            crate::nanbox_ffi::__torajs_anyv_rc_dec(__torajs_anyv_box_pointer(s));
            *iter_slot = __torajs_anyv_box_pointer(flat.cast::<c_void>());
        }
        let s = as_void_ptr(*iter_slot).cast::<u8>();
        let mut adv: i64 = 0;
        let ch = __torajs_str_code_point_str_at(s, *idx_slot, &mut adv);
        if ch.is_null() {
            *out = VALUE_UNDEFINED;
            return 0;
        }
        *idx_slot += adv;
        *out = __torajs_anyv_box_pointer(ch.cast::<c_void>());
        // §27.1.4.4 — a sync lane's value is awaited under
        // `for await` (a string never yields a thenable, but the
        // tick count is the protocol's).
        if mode.is_async() {
            return crate::iter_any_await::settle_out(out, mode);
        }
        1
    }
}
