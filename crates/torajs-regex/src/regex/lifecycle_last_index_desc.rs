//! `lastIndex` as a PROPERTY — `defineProperty` over it and the
//! writable bit `getOwnPropertyDescriptor` reports. Split from
//! `lifecycle` under the 500-line file rule.
//!
//! The seam is the question each side answers: `lifecycle` holds what
//! the slot IS (drop, source, flags, the numeric get / set), this
//! holds what the descriptor machinery may do TO it — §10.1.6.3
//! ValidateAndApplyPropertyDescriptor over a slot that is not an
//! ordinary entry.

use core::ffi::c_void;

use super::lifecycle::last_index_locked;
use super::{as_regex, as_regex_mut};

/// §22.2.4.1's `lastIndex` is the ONE own property a RegExp keeps
/// in its cell rather than in the lazy bag, and it is minted
/// `{writable: true, enumerable: false, configurable: false}`. This
/// is its [[DefineOwnProperty]]: §10.1.6.3 against that descriptor,
/// then the store. Before it existed, a define naming `lastIndex`
/// reached the "no expando define storage" arm and was dropped in
/// silence — `Object.defineProperty(re, "lastIndex", {value: 3})`
/// left the old value in place and answered as if it had worked.
///
/// `flags_byte` is torajs-dynobj's `DEFINE_*` encoding. Answers 1
/// when the define is accepted, 0 when §10.1.6.3 rejects it (the
/// caller flavors the TypeError). The `(tag, value)` pair transfers
/// on acceptance; on a rejection the caller keeps it and releases.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`; `value` carries the
/// caller's +1 on a heap payload.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_last_index_define(
    re_ptr: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
) -> i64 {
    if re_ptr.is_null() {
        return 0;
    }
    let re = unsafe { as_regex_mut(re_ptr) };
    let writable = !last_index_locked(re);
    // An accessor descriptor against a data property whose
    // [[Configurable]] is false — §10.1.6.3 step 3.
    if flags_byte & (DEFINE_PRESENT_GET | DEFINE_PRESENT_SET) != 0 {
        return 0;
    }
    // Step 4.a — [[Configurable]] and [[Enumerable]] are fixed, so
    // asking for anything but what they already are is a refusal.
    if flags_byte & DEFINE_PRESENT_CONFIGURABLE != 0 && flags_byte & DEFINE_FLAG_CONFIGURABLE != 0 {
        return 0;
    }
    if flags_byte & DEFINE_PRESENT_ENUMERABLE != 0 && flags_byte & DEFINE_FLAG_ENUMERABLE != 0 {
        return 0;
    }
    // §10.1.6.3 compares only the fields the descriptor HAS. A
    // descriptor with no [[Writable]] is not a request for
    // `writable: true`.
    let asks_writable_true =
        flags_byte & DEFINE_PRESENT_WRITABLE != 0 && flags_byte & DEFINE_FLAG_WRITABLE != 0;
    let asks_writable_false =
        flags_byte & DEFINE_PRESENT_WRITABLE != 0 && flags_byte & DEFINE_FLAG_WRITABLE == 0;
    if !writable {
        // Step 4.b — a non-configurable, non-writable property takes
        // neither its writability back nor a DIFFERENT value.
        // Redefining it with the value it already holds is a no-op
        // the spec accepts (SameValue), and test262 leans on that.
        if asks_writable_true {
            return 0;
        }
        if flags_byte & DEFINE_PRESENT_VALUE != 0 && !unsafe { holds_same_value(re, tag, value) } {
            return 0;
        }
        return 1;
    }
    if flags_byte & DEFINE_PRESENT_VALUE != 0 {
        if tag == ANY_I64 || tag == ANY_F64 {
            let n = if tag == ANY_I64 {
                value as i64 as f64
            } else {
                f64::from_bits(value)
            };
            re.set_last_index_num(n);
        } else {
            // Verbatim, like the any-lane assignment: ToLength runs
            // at the §22.2.7.2 exec entry, not here. Spelled against
            // the field rather than through the extern store so the
            // freeze below is not ordering-sensitive.
            if re.last_index_boxed != 0 {
                unsafe { super::__torajs_value_drop_heap(re.last_index_boxed as *mut c_void) };
            }
            re.last_index_boxed = value;
        }
    }
    if asks_writable_false {
        unsafe { as_regex_mut(re_ptr) }.last_index_frozen = 1;
    }
    1
}

/// SameValue between the stored `lastIndex` and an incoming
/// `(tag, value)` — §10.1.6.3 step 4's escape hatch for redefining
/// a frozen property with the value it already holds. NaN equals
/// NaN here and `+0` does not equal `-0`, which is SameValue and
/// not `===`.
///
/// # Safety
/// `re` is a live `RegExp`.
unsafe fn holds_same_value(re: &super::RegExp, tag: u64, value: u64) -> bool {
    let incoming_num = match tag {
        ANY_I64 => Some(value as i64 as f64),
        ANY_F64 => Some(f64::from_bits(value)),
        _ => None,
    };
    if re.last_index_boxed != 0 {
        // A boxed store is compared verbatim: the same NaN-box bits
        // are the same value, and a numeric request against a boxed
        // slot is a different value by construction (the numeric
        // form would have reset the box).
        return incoming_num.is_none() && re.last_index_boxed == value;
    }
    let Some(n) = incoming_num else {
        return false;
    };
    let cur = re.last_index;
    (cur.is_nan() && n.is_nan()) || cur.to_bits() == n.to_bits()
}

/// Whether `lastIndex` is still writable — the bit
/// `getOwnPropertyDescriptor` reports and the assignment kernels
/// refuse on.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_last_index_writable(re_ptr: *const c_void) -> i64 {
    if re_ptr.is_null() {
        return 1;
    }
    (!last_index_locked(unsafe { as_regex(re_ptr) })) as i64
}

/// Whether nothing on the cell itself can tell the §22.2.6 protocol
/// methods' spec algorithms from the native kernels: no own property
/// bag (an own `exec`, `flags`, `global` or `constructor` lives
/// there), no subclass prototype between the cell and
/// %RegExp.prototype%, a writable `lastIndex`, and a numeric one (a
/// boxed value may carry a `valueOf` the spec's ToLength calls).
/// 1 = plain, 0 = the caller runs the spec algorithm. What the
/// PROTOTYPE carries is the caller's half of the question.
///
/// # Safety
/// `re_ptr` is a live `*RegExp`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_protocol_plain(re_ptr: *const c_void) -> i64 {
    let re = unsafe { as_regex(re_ptr) };
    let plain = re.props.is_null()
        && re.header.flags & super::subclass::FLAG_SUBCLASSED == 0
        && !last_index_locked(re)
        && re.last_index_boxed == 0;
    plain as i64
}

/// `DEFINE_*` mirrors — torajs-dynobj `layout.rs` is the source.
const DEFINE_FLAG_WRITABLE: u64 = 1 << 0;
const DEFINE_FLAG_ENUMERABLE: u64 = 1 << 1;
const DEFINE_FLAG_CONFIGURABLE: u64 = 1 << 2;
const DEFINE_PRESENT_WRITABLE: u64 = 1 << 3;
const DEFINE_PRESENT_ENUMERABLE: u64 = 1 << 4;
const DEFINE_PRESENT_CONFIGURABLE: u64 = 1 << 5;
const DEFINE_PRESENT_VALUE: u64 = 1 << 6;
const DEFINE_PRESENT_GET: u64 = 1 << 7;
const DEFINE_PRESENT_SET: u64 = 1 << 8;

/// `ANY_TAG` mirrors for the numeric fast slot.
const ANY_I64: u64 = 2;
const ANY_F64: u64 = 3;
