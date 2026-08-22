//! §25.1.6 `ArrayBuffer.prototype` accessors, read through the
//! any-lane member probe (RFC 20260823-typedarray-substrate 刀 1).
//!
//! These are prototype accessors, not own properties, so this answers
//! a **[[Get]]** — which walks the chain — and deliberately says
//! nothing about own-key enumeration. `Object.keys(buf)` stays empty
//! because the own-keys kernel is a different question and is not
//! touched here.
//!
//! The substrate lives in `torajs-buffer`, which depends on this
//! crate for the NaN-box encoding; the edge back is therefore
//! `extern "C"` only. Each getter re-checks the receiver tag, which
//! is a load and a compare — cheap enough that duplicating the
//! predicate on this side would buy nothing but a second place for
//! it to drift.

use core::ffi::c_void;

use torajs_rc::AnySlotTag;

use crate::nanbox::AnyValue;

unsafe extern "C" {
    fn __torajs_arraybuffer_byte_length(av: AnyValue) -> i64;
    fn __torajs_arraybuffer_max_byte_length(av: AnyValue) -> i64;
    fn __torajs_arraybuffer_resizable(av: AnyValue) -> i64;
    fn __torajs_arraybuffer_detached(av: AnyValue) -> i64;
}

/// The `(tag, value)` probe pair for an ArrayBuffer receiver, or
/// `None` when the key names none of the four accessors and the read
/// belongs to the builtin-method reify.
///
/// # Safety
/// `recv` is a live ArrayBuffer AnyValue; `key` is NULL or a live Str
/// cell.
pub(crate) unsafe fn arraybuffer_prop(recv: AnyValue, key: *const c_void) -> Option<(u64, u64)> {
    unsafe {
        if crate::prop_has::key_is(key, b"byteLength") {
            return Some(num(__torajs_arraybuffer_byte_length(recv)));
        }
        if crate::prop_has::key_is(key, b"maxByteLength") {
            return Some(num(__torajs_arraybuffer_max_byte_length(recv)));
        }
        if crate::prop_has::key_is(key, b"resizable") {
            return Some(boolean(__torajs_arraybuffer_resizable(recv)));
        }
        if crate::prop_has::key_is(key, b"detached") {
            return Some(boolean(__torajs_arraybuffer_detached(recv)));
        }
        None
    }
}

/// A byte count on the probe pair. JS numbers are doubles and the
/// F64 channel is the one that never has to decide whether a count
/// is small enough for the integer lane.
#[inline]
fn num(n: i64) -> (u64, u64) {
    (AnySlotTag::F64 as u64, (n as f64).to_bits())
}

#[inline]
fn boolean(b: i64) -> (u64, u64) {
    (AnySlotTag::Bool as u64, u64::from(b != 0))
}
