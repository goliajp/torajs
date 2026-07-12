//! Array cell gOPD arm — RFC 20260712-arr-exotic-define chunk A.
//!
//! `Object.getOwnPropertyDescriptor(arr, key)` for a `Tag::Arr` cell
//! behind an `any` view. Pre-fix the runtime walk answered
//! `undefined` for every Array receiver (reflect.rs fell through the
//! `htag != TAG_DYNOBJ` guard), which crashed test262's
//! `verifyProperty` at `originalDesc.value` — the dominant
//! "cannot read properties of null or undefined" failure cluster.
//!
//! Three arms per spec §10.4.2:
//! - `"length"` → `{value: len, writable: true, enumerable: false,
//!   configurable: false}` (§10.4.2.4; the writable bit reads the
//!   length-RO lock once chunk D lands it).
//! - canonical array index (§7.1.22 shape, `< 2^32-1`) → in-range
//!   answers `{value: elem, writable/enumerable/configurable: true}`
//!   (defaults; chunk B threads the per-index shadow flags through);
//!   out-of-range answers `undefined` — the index domain is owned by
//!   element storage, never by the expando dynobj.
//! - anything else → expando props dynobj walk (same delegation shape
//!   as `closure_reflect`: recurse into the main gOPD entry with the
//!   props dynobj as receiver so accessor entries / attribute flags
//!   ride the existing TAG_DYNOBJ arm).

use core::ffi::c_void;

use crate::reflect::{VALUE_UNDEFINED_IMM, build_data_descriptor};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const u8) -> bool;
    /// torajs-arr kind-aware slot reads — borrow contract (no inc).
    fn __torajs_arr_get_any_tag(arr: *const c_void, i: u64) -> u64;
    fn __torajs_arr_get_any_value(arr: *const c_void, i: u64) -> u64;
}

/// Array heap layout mirror (`torajs-arr::layout`): len u64 at +8,
/// expando props-dynobj slot at +24.
const ARR_LEN_OFF: usize = 8;
const ARR_PROPS_OFF: usize = 24;

/// Key Str layout mirror — len u32 at +8, payload at +16.
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;

/// `AnySlotTag::Heap` mirror — heap-tagged descriptor values owe an
/// inc (the slot keeps its reference, the descriptor owns a fresh one).
const ANY_HEAP: u64 = 4;

/// Key Str payload as a byte slice.
unsafe fn key_bytes<'a>(key: *const c_void) -> &'a [u8] {
    let len = unsafe { key.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() };
    unsafe { core::slice::from_raw_parts(key.cast::<u8>().add(STR_DATA_OFF), len as usize) }
}

/// Canonical array-index parse — ES §10.4.2 array index: a canonical
/// numeric string (`"0"`, or nonzero-leading all-digits) whose value
/// is `< 2^32 - 1`. Anything else (leading zero, sign, empty, huge)
/// is an ordinary property key.
fn canonical_index(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() || bytes.len() > 10 {
        return None;
    }
    if bytes == b"0" {
        return Some(0);
    }
    if bytes[0] == b'0' || !bytes.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut v: u64 = 0;
    for &b in bytes {
        v = v * 10 + (b - b'0') as u64;
    }
    if v < u32::MAX as u64 { Some(v) } else { None }
}

/// See module doc.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer (caller checked the header
/// tag); `key` is a live `Str` pointer (caller checked non-NULL).
pub(crate) unsafe fn arr_cell_descriptor(arr: *const c_void, key: *const c_void) -> u64 {
    let bytes = unsafe { key_bytes(key) };
    if bytes == b"length" {
        // §10.4.2.4 — {value: len, writable: true, enumerable: false,
        // configurable: false}. AnySlotTag::I64 = 2.
        let len = unsafe { arr.cast::<u8>().add(ARR_LEN_OFF).cast::<u64>().read() };
        return unsafe { build_data_descriptor(2, len, 1, 0, 0) };
    }
    if let Some(idx) = canonical_index(bytes) {
        let len = unsafe { arr.cast::<u8>().add(ARR_LEN_OFF).cast::<u64>().read() };
        if idx >= len {
            return VALUE_UNDEFINED_IMM;
        }
        let tag = unsafe { __torajs_arr_get_any_tag(arr, idx) };
        let val = unsafe { __torajs_arr_get_any_value(arr, idx) };
        if tag == ANY_HEAP && val != 0 {
            // SAFETY: ANY_HEAP slot holds a valid heap pointer — the
            // element keeps its share, the descriptor owns a fresh one.
            unsafe { __torajs_rc_inc(val as *mut c_void) };
        }
        return unsafe { build_data_descriptor(tag, val, 1, 1, 1) };
    }
    // Expando walk — delegate to the main gOPD entry with the props
    // dynobj as receiver (rides the TAG_DYNOBJ arm; the has-gate keeps
    // the builtin-proto probes from misfiring on an instance dynobj).
    let props = unsafe {
        arr.cast::<u8>()
            .add(ARR_PROPS_OFF)
            .cast::<*const c_void>()
            .read()
    };
    if !props.is_null() && unsafe { __torajs_dynobj_has(props, key as *const u8) } {
        return unsafe { crate::reflect::__torajs_anyv_get_property_descriptor(props as u64, key) };
    }
    VALUE_UNDEFINED_IMM
}

#[cfg(test)]
mod tests {
    use super::canonical_index;

    #[test]
    fn canonical_index_accepts_plain_digits() {
        assert_eq!(canonical_index(b"0"), Some(0));
        assert_eq!(canonical_index(b"7"), Some(7));
        assert_eq!(canonical_index(b"4294967294"), Some(4294967294));
    }

    #[test]
    fn canonical_index_rejects_non_canonical() {
        assert_eq!(canonical_index(b""), None);
        assert_eq!(canonical_index(b"00"), None);
        assert_eq!(canonical_index(b"01"), None);
        assert_eq!(canonical_index(b"-1"), None);
        assert_eq!(canonical_index(b"1.5"), None);
        assert_eq!(canonical_index(b"length"), None);
        // 2^32 - 1 is NOT an array index (it is the length ceiling).
        assert_eq!(canonical_index(b"4294967295"), None);
        assert_eq!(canonical_index(b"99999999999"), None);
    }
}
