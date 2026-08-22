//! The generic [[Get]] the Proxy substrate reads handlers and
//! targets through (RFC 20260823-proxy-substrate 刀 1).
//!
//! Same recipe as `iter_any_result::iter_result_get` — member-get
//! pair, accessor sentinel routed through the single receiver-aware
//! kernel, owned result — but keyed by an existing key cell as well
//! as by a name, because a `get` trap forwards whatever key the read
//! used (`Symbol.iterator` included).
//!
//! Answers OWNED. `None` = a getter (or a nested proxy trap) threw
//! and the pending throw is already recorded.

use core::ffi::c_void;

use crate::nanbox::AnyValue;
use crate::nanbox_encode::__torajs_anyv_box_from_pair;
use crate::payload_rc_inc;

unsafe extern "C" {
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_throw_check() -> i64;
}

/// [[Get]] on `recv` for an existing key cell (Str or Symbol).
///
/// # Safety
/// `recv` is a live AnyValue; `key` a live Str or Symbol cell that
/// outlives the call.
pub(crate) unsafe fn get_by_key_checked(recv: AnyValue, key: *const c_void) -> Option<AnyValue> {
    unsafe {
        let tag = crate::member_get::__torajs_any_member_get_tag(recv, key);
        if __torajs_throw_check() != 0 {
            return None;
        }
        if tag == crate::struct_probe::ANY_ACCESSOR_TAG {
            let pair_bits = crate::member_get_value::__torajs_any_member_get_value(recv, key);
            let got = crate::struct_probe::__torajs_any_accessor_get(recv, key, pair_bits);
            if __torajs_throw_check() != 0 {
                return None;
            }
            return Some(got);
        }
        // The probe pair is a borrow off the receiver — take the
        // stake before boxing, so every arm here answers owned.
        let payload = crate::member_get_value::__torajs_any_member_get_value(recv, key);
        payload_rc_inc(tag as i64, payload as i64);
        Some(__torajs_anyv_box_from_pair(tag as i64, payload as i64))
    }
}

/// [[Get]] on `recv` for an existing key cell, answering `undefined`
/// where the checked form answers `None` — the forwarding arm of a
/// trap-less internal method, whose caller propagates the pending
/// throw through its own check.
///
/// # Safety
/// See [`get_by_key_checked`].
pub(crate) unsafe fn get_by_key(recv: AnyValue, key: *const c_void) -> AnyValue {
    unsafe { get_by_key_checked(recv, key).unwrap_or(crate::nanbox::VALUE_UNDEFINED) }
}

/// [[Get]] on `recv` for a plain ASCII name — mints the Str key,
/// reads, releases it.
///
/// # Safety
/// `recv` is a live AnyValue.
pub(crate) unsafe fn get_by_name(recv: AnyValue, name: &[u8]) -> Option<AnyValue> {
    unsafe {
        let key = __torajs_str_alloc(name.as_ptr(), name.len() as i64) as *const c_void;
        let out = get_by_key_checked(recv, key);
        __torajs_str_drop(key as *mut c_void);
        out
    }
}
