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
    /// torajs-arr — per-index attribute flags (shadow entry or the
    /// implicit `w|e|c` defaults; chunk B).
    fn __torajs_arr_index_flags(arr: *const c_void, idx: u64) -> u64;
    /// torajs-arr — the arguments-materialization `"length"` face:
    /// 0 = plain array, 1 = arguments (configurable), 2 = deleted.
    fn __torajs_arr_arguments_length_state(arr: *const c_void, key: *const c_void) -> i64;
    /// torajs-arr — bare FLAG_ARR_ARGUMENTS probe (the callee arm).
    fn __torajs_arr_is_arguments(arr: *const c_void) -> i64;
    /// torajs-rc — the interned §10.2.4 %ThrowTypeError% method cell.
    fn __torajs_builtin_method_cell(mid: i64) -> *mut u8;
    /// torajs-arr — the index's AccessorPair, NULL when not an
    /// accessor (RFC 20260713 chunk C).
    fn __torajs_arr_index_accessor(arr: *const c_void, idx: u64) -> *mut c_void;
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

/// `arr_index_flags` result bit 3 — deleted index (hole;
/// `torajs_arr::define::F_HOLE` mirror, RFC 20260713 chunk C).
const ARR_F_HOLE: u64 = 1 << 3;

/// `torajs_rc::FLAG_ARR_LENGTH_RO` mirror (Tag::Arr-private bit 7;
/// this crate keeps its dep tree narrow — the u16 bit position is
/// part of the header ABI).
const ARR_FLAG_LENGTH_RO: u16 = 1 << 7;

/// Mirror of `torajs_rc::ANY_METHOD_THROW_TYPE_ERROR` (the
/// `object_proto_install` sibling keeps the same local mirror).
const ANY_METHOD_THROW_TYPE_ERROR_MID: i64 = 155;

/// Key Str payload as a byte slice.
unsafe fn key_bytes<'a>(key: *const c_void) -> &'a [u8] {
    let len = unsafe { key.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() };
    unsafe { core::slice::from_raw_parts(key.cast::<u8>().add(STR_DATA_OFF), len as usize) }
}

/// Canonical array-index parse — ES §10.4.2 array index: a canonical
/// numeric string (`"0"`, or nonzero-leading all-digits) whose value
/// is `< 2^32 - 1`. Anything else (leading zero, sign, empty, huge)
/// is an ordinary property key. RFC 20260716 刀 16 reuses this from
/// the StringWrapper char-index descriptor arm — the shape is spec
/// §7.1.22 CanonicalNumericIndexString and applies identically.
pub(crate) fn canonical_index(bytes: &[u8]) -> Option<u64> {
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
        // §10.4.2.4 — {value: len, writable: !locked, enumerable:
        // false, configurable: false}. AnySlotTag::I64 = 2; the lock
        // is FLAG_ARR_LENGTH_RO (Tag::Arr-private bit 7 — bits 13-14
        // are the cycle-collector color field, RFC 20260713 chunk A).
        // An arguments materialization (§10.4.4) carries a PLAIN
        // data length instead — configurable true, and a deleted one
        // has no own entry at all (the hole tombstone; the state
        // kernel reads it off the expando bag).
        let args_state = unsafe { __torajs_arr_arguments_length_state(arr, key) };
        if args_state == 2 {
            return VALUE_UNDEFINED_IMM;
        }
        let len = unsafe { arr.cast::<u8>().add(ARR_LEN_OFF).cast::<u64>().read() };
        let flags = unsafe { arr.cast::<u8>().add(6).cast::<u16>().read() };
        let writable = (flags & ARR_FLAG_LENGTH_RO == 0) as u64;
        let configurable = (args_state == 1) as u64;
        return unsafe { build_data_descriptor(2, len, writable, 0, configurable) };
    }
    if bytes == b"callee" && unsafe { __torajs_arr_is_arguments(arr) } != 0 {
        // §10.4.4.6 CreateUnmappedArgumentsObject step 21 — `callee`
        // is the %ThrowTypeError% accessor pair, enumerable false,
        // configurable false (tr's module goal is always strict /
        // unmapped). The interned thrower cell is immortal — the
        // descriptor's stake no-ops.
        let thrower = unsafe { __torajs_builtin_method_cell(ANY_METHOD_THROW_TYPE_ERROR_MID) };
        return unsafe {
            crate::reflect::build_accessor_descriptor(4, thrower as u64, 4, thrower as u64, 0, 0)
        };
    }
    if let Some(idx) = canonical_index(bytes) {
        let len = unsafe { arr.cast::<u8>().add(ARR_LEN_OFF).cast::<u64>().read() };
        if idx >= len {
            return VALUE_UNDEFINED_IMM;
        }
        // A deleted index (hole shadow entry, chunk C) is absent.
        if unsafe { __torajs_arr_index_flags(arr, idx) } & ARR_F_HOLE != 0 {
            return VALUE_UNDEFINED_IMM;
        }
        // An accessor index (RFC 20260713 chunk C) — its shadow entry
        // IS a dynobj accessor entry; delegate to the main gOPD with
        // the props dynobj as receiver so the TAG_DYNOBJ arm reports
        // {get, set, enumerable, configurable}.
        if unsafe { !__torajs_arr_index_accessor(arr, idx).is_null() } {
            let props = unsafe {
                arr.cast::<u8>()
                    .add(ARR_PROPS_OFF)
                    .cast::<*const c_void>()
                    .read()
            };
            return unsafe {
                crate::reflect_get_property_descriptor::__torajs_anyv_get_property_descriptor(
                    props as u64,
                    key,
                )
            };
        }
        let tag = unsafe { __torajs_arr_get_any_tag(arr, idx) };
        let val = unsafe { __torajs_arr_get_any_value(arr, idx) };
        if tag == ANY_HEAP && val != 0 {
            // SAFETY: ANY_HEAP slot holds a valid heap pointer — the
            // element keeps its share, the descriptor owns a fresh one.
            unsafe { __torajs_rc_inc(val as *mut c_void) };
        }
        // Shadow entry flags (or the implicit defaults) — bits 0/1/2
        // = writable / enumerable / configurable.
        let flags = unsafe { __torajs_arr_index_flags(arr, idx) };
        return unsafe {
            build_data_descriptor(tag, val, flags & 1, (flags >> 1) & 1, (flags >> 2) & 1)
        };
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
        return unsafe {
            crate::reflect_get_property_descriptor::__torajs_anyv_get_property_descriptor(
                props as u64,
                key,
            )
        };
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
