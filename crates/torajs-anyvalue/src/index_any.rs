//! `__torajs_any_index_get` — `recv[idx]` where the receiver is an
//! `any` value (Any-dynamic-access RFC 20260704 S3).
//!
//! Dispatch tree (ES ordinary property access semantics):
//! - `null` / `undefined` receiver → catchable TypeError (ES §13.3.2
//!   RequireObjectCoercible), returns `undefined` after recording the
//!   pending throw (caller's `emit_throw_check` propagates).
//! - numeric / bool immediates → `undefined` (primitives have no
//!   own indexed properties).
//! - ShortStr — ASCII payloads answer inline (byte == code unit);
//!   non-ASCII payloads materialize to a heap Str and reuse the heap
//!   path so UTF-16 code-unit semantics match the typed tier.
//! - `Tag::Str` cell (Str or Substr view) →
//!   `__torajs_str_index_get` (torajs-str); NULL = OOB → `undefined`.
//! - `Tag::Arr` cell → `__torajs_arr_index_get` (torajs-arr,
//!   kind-aware: FLAG_ARR_ANY NaN-box slots or `ARR_KIND_*` raw
//!   slots recorded at the boxing boundary).
//! - `Tag::DynObj` → explicit TypeError — numeric-key property
//!   lookup on plain objects is the RFC's S4 follow-up (a roadmap
//!   boundary, never a silent wrong answer).
//! - any other heap tag → `undefined` (no such own property).

use core::ffi::c_void;

use crate::nanbox::{
    AnyValue, VALUE_UNDEFINED, as_void_ptr, box_void_ptr, is_bool, is_cell, is_double, is_int32,
    is_null, is_short_str, is_undefined, short_str_bytes, short_str_len, try_box_short_str,
};
use crate::nanbox_ffi_materialize::materialize_short_str;
use torajs_rc::Tag;

unsafe extern "C" {
    /// torajs-str — `s[idx]` (Str or Substr); NULL = OOB.
    fn __torajs_str_index_get(s: *mut u8, idx: i64) -> *mut u8;
    /// torajs-arr — kind-aware `arr[idx]`; returns a balanced
    /// AnyValue (+1 for cells).
    fn __torajs_arr_index_get(arr: *const c_void, idx: i64) -> u64;
    /// torajs-arr — kind-aware `arr[idx] = (tag, value)` (pair ABI,
    /// tag 4 transfers one rc).
    fn __torajs_arr_index_set(arr: *mut c_void, idx: i64, tag: u64, value: u64);
    /// Universal NaN-box-safe heap dropper (torajs-value-drop).
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-str — release a heap Str/Substr reference. Signature
    /// mirrors the crate-local test stub (`*mut c_void`).
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-throw — record a pending catchable TypeError; returns
    /// normally (caller's throw-check propagates).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-str — heap Str constructor (rc=1), used to build the
    /// literal "length" probe key for DynObj receivers.
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// torajs-dynobj — own-property probe pair ((5, 0) = absent →
    /// undefined by construction).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-collections — live entry count (tombstones excluded);
    /// Set storage shares the Map runtime.
    fn __torajs_map_size(p: *const c_void) -> i64;
}

/// See module doc.
///
/// # Safety
/// Cell receivers must be valid heap pointers matching their header
/// tag layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_index_get(recv: AnyValue, idx: i64) -> AnyValue {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot read properties of null or undefined".as_ptr());
        }
        return VALUE_UNDEFINED;
    }
    if is_short_str(recv) {
        let len = short_str_len(recv) as usize;
        let bytes = short_str_bytes(recv);
        let payload = &bytes[..len];
        if payload.iter().all(|b| *b < 0x80) {
            // ASCII: byte index == UTF-16 code-unit index.
            if idx < 0 || idx as usize >= len {
                return VALUE_UNDEFINED;
            }
            return try_box_short_str(&payload[idx as usize..idx as usize + 1])
                .unwrap_or(VALUE_UNDEFINED);
        }
        // Non-ASCII payload: materialize to a heap Str and reuse the
        // heap path so code-unit semantics match the typed tier. The
        // returned Substr holds its own parent reference, so the
        // temporary parent drops safely right after.
        unsafe {
            let parent = materialize_short_str(recv);
            let out = index_str_cell(parent, idx);
            __torajs_str_drop(parent as *mut c_void);
            return out;
        }
    }
    if is_int32(recv) || is_double(recv) || is_bool(recv) {
        return VALUE_UNDEFINED;
    }
    if !is_cell(recv) {
        return VALUE_UNDEFINED;
    }
    let ptr = as_void_ptr(recv);
    let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
    if tag == Tag::Str as u16 {
        return unsafe { index_str_cell(ptr as *mut u8, idx) };
    }
    if tag == Tag::Arr as u16 {
        return unsafe { __torajs_arr_index_get(ptr, idx) };
    }
    if tag == Tag::DynObj as u16 {
        unsafe {
            __torajs_throw_type_error(
                c"indexing a plain object through any is not yet implemented".as_ptr(),
            );
        }
        return VALUE_UNDEFINED;
    }
    VALUE_UNDEFINED
}

/// Shared `Tag::Str` arm — `str_index_get` returns a fresh rc=1
/// Substr (ownership transfers into the box) or NULL for OOB.
unsafe fn index_str_cell(s: *mut u8, idx: i64) -> AnyValue {
    unsafe {
        let sub = __torajs_str_index_get(s, idx);
        if sub.is_null() {
            VALUE_UNDEFINED
        } else {
            box_void_ptr(sub as *mut c_void)
        }
    }
}

/// `recv[idx] = (tag, value)` where the receiver is an `any` value
/// (RFC 20260704 S3-set). Pair ABI mirrors ssa-lower's
/// `pack_any_slot_value` — for `tag == 4` the caller transfers
/// ownership of one rc; every path that doesn't store the pair
/// releases it.
///
/// - `null` / `undefined` receiver → catchable TypeError.
/// - primitive receivers (numbers / bools / strings — including
///   heap Str/Substr cells) → silent no-op, matching non-strict JS
///   assignment-to-primitive-property semantics (§13.15.2 PutValue on
///   a primitive base discards in sloppy mode; strings are immutable
///   either way).
/// - `Tag::Arr` cell → `__torajs_arr_index_set` (kind-aware; OOB →
///   catchable RangeError, kind mismatch → catchable TypeError).
/// - `Tag::DynObj` / any other heap tag → explicit TypeError (the
///   numeric-key property write is the RFC's S4 follow-up).
///
/// # Safety
/// Cell receivers must be valid heap pointers matching their header
/// tag layout; a `tag == 4` `value` must be 0 or a valid owned heap
/// pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_index_set(recv: AnyValue, idx: i64, tag: u64, value: u64) {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            drop_transferred_pair(tag, value);
            __torajs_throw_type_error(c"cannot set properties of null or undefined".as_ptr());
        }
        return;
    }
    if !is_cell(recv) {
        unsafe { drop_transferred_pair(tag, value) };
        return;
    }
    let ptr = as_void_ptr(recv);
    let hdr_tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
    if hdr_tag == Tag::Arr as u16 {
        unsafe { __torajs_arr_index_set(ptr, idx, tag, value) };
        return;
    }
    if hdr_tag == Tag::Str as u16 {
        unsafe { drop_transferred_pair(tag, value) };
        return;
    }
    unsafe {
        drop_transferred_pair(tag, value);
        __torajs_throw_type_error(
            c"indexed write on this receiver through any is not yet implemented".as_ptr(),
        );
    }
}

/// Release a transferred `tag == 4` rc (no-op for immediates).
unsafe fn drop_transferred_pair(tag: u64, value: u64) {
    if tag == 4 {
        unsafe { __torajs_value_drop_heap(value as *mut c_void) };
    }
}

// Layout mirrors for the length fast paths (kept inline per the
// one-constant-per-crate convention, same as torajs-arr's ANY_HEAP):
// torajs-arr `ARR_LEN_OFF` / torajs-str `STR_LEN_OFF` (u32 code
// units) / torajs-str `SUBSTR_LEN_OFF` (u64 code units) /
// `FLAG_SUBSTR_INLINE` (HeapHeader flags bit 0).
const MIRROR_ARR_LEN_OFF: usize = 8;
const MIRROR_STR_LEN_OFF: usize = 8;
const MIRROR_SUBSTR_LEN_OFF: usize = 8;
const MIRROR_FLAG_SUBSTR_INLINE: u16 = 1 << 0;

/// `recv.length` where the receiver is an `any` value
/// (RFC 20260704 S4).
///
/// - `null` / `undefined` → catchable TypeError.
/// - strings (ShortStr / Str / Substr) → UTF-16 code-unit count.
/// - `Tag::Arr` → element count.
/// - `Tag::DynObj` → own-property probe for the literal key
///   `"length"` (a user `{ length: 5 }` answers 5; absence answers
///   `undefined`).
/// - other primitives / heap tags → `undefined` (no such property).
///
/// # Safety
/// Cell receivers must be valid heap pointers matching their header
/// tag layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_length_get(recv: AnyValue) -> AnyValue {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot read properties of null or undefined".as_ptr());
        }
        return VALUE_UNDEFINED;
    }
    if is_short_str(recv) {
        let len = short_str_len(recv) as usize;
        let bytes = short_str_bytes(recv);
        let payload = &bytes[..len];
        let units = if payload.iter().all(|b| *b < 0x80) {
            len as i64
        } else {
            utf16_units_of_utf8(payload)
        };
        return crate::nanbox_encode::__torajs_anyv_box_i64(units);
    }
    if !is_cell(recv) {
        return VALUE_UNDEFINED;
    }
    let ptr = as_void_ptr(recv);
    unsafe {
        let tag = (ptr.cast::<u8>().add(4) as *const u16).read();
        if tag == Tag::Arr as u16 {
            let n = *(ptr.cast::<u8>().add(MIRROR_ARR_LEN_OFF) as *const u64);
            return crate::nanbox_encode::__torajs_anyv_box_i64(n as i64);
        }
        if tag == Tag::Str as u16 {
            let flags = (ptr.cast::<u8>().add(6) as *const u16).read();
            let n = if flags & MIRROR_FLAG_SUBSTR_INLINE != 0 {
                *(ptr.cast::<u8>().add(MIRROR_SUBSTR_LEN_OFF) as *const u64) as i64
            } else {
                *(ptr.cast::<u8>().add(MIRROR_STR_LEN_OFF) as *const u32) as i64
            };
            return crate::nanbox_encode::__torajs_anyv_box_i64(n);
        }
        if tag == Tag::DynObj as u16 {
            // Own-property probe for the literal key "length" — a
            // user `{ length: 5 }` through any answers its value;
            // absence answers (5, 0) = undefined by construction.
            let key = __torajs_str_alloc(c"length".as_ptr() as *const u8, 6);
            let dtag = __torajs_dynobj_get_tag(ptr, key as *const c_void);
            let dval = __torajs_dynobj_get_value(ptr, key as *const c_void);
            __torajs_str_drop(key as *mut c_void);
            // The probe pair is a borrow — the returned box owns its
            // own reference (same shape as the lower-side dynobj
            // fallback's payload_rc_inc + any_box pairing).
            crate::payload_rc_inc(dtag as i64, dval as i64);
            return crate::nanbox_encode::__torajs_anyv_box_from_pair(dtag as i64, dval as i64);
        }
    }
    VALUE_UNDEFINED
}

/// `recv.size` where the receiver is an `any` value
/// (Any-method-call RFC 20260704 C4-2).
///
/// - `null` / `undefined` → catchable TypeError.
/// - `Tag::Map` / `Tag::Set` → live entry count (the ES §24.1.3.10 /
///   §24.2.3.9 `size` accessors) via `__torajs_map_size`.
/// - `Tag::DynObj` → own-property probe for the literal key `"size"`
///   (a user `{ size: 42 }` answers 42; absence answers `undefined`).
/// - everything else (strings, arrays, primitives, other heap tags)
///   → `undefined` (no such property).
///
/// # Safety
/// Cell receivers must be valid heap pointers matching their header
/// tag layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_size_get(recv: AnyValue) -> AnyValue {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot read properties of null or undefined".as_ptr());
        }
        return VALUE_UNDEFINED;
    }
    if !is_cell(recv) {
        return VALUE_UNDEFINED;
    }
    let ptr = as_void_ptr(recv);
    unsafe {
        let tag = (ptr.cast::<u8>().add(4) as *const u16).read();
        if tag == Tag::Map as u16 || tag == Tag::Set as u16 {
            return crate::nanbox_encode::__torajs_anyv_box_i64(__torajs_map_size(ptr));
        }
        if tag == Tag::DynObj as u16 {
            // Same probe shape as the `.length` DynObj arm above —
            // the pair is a borrow, the returned box owns its own
            // reference.
            let key = __torajs_str_alloc(c"size".as_ptr() as *const u8, 4);
            let dtag = __torajs_dynobj_get_tag(ptr, key as *const c_void);
            let dval = __torajs_dynobj_get_value(ptr, key as *const c_void);
            __torajs_str_drop(key as *mut c_void);
            crate::payload_rc_inc(dtag as i64, dval as i64);
            return crate::nanbox_encode::__torajs_anyv_box_from_pair(dtag as i64, dval as i64);
        }
    }
    VALUE_UNDEFINED
}

/// Count UTF-16 code units of a UTF-8 byte payload (≤ 5 bytes for a
/// ShortStr, so a simple lead-byte walk suffices; 4-byte sequences
/// decode to astral cps = 2 units).
fn utf16_units_of_utf8(payload: &[u8]) -> i64 {
    let mut units = 0i64;
    let mut i = 0usize;
    while i < payload.len() {
        let b = payload[i];
        let (adv, u) = if b < 0x80 {
            (1, 1)
        } else if b < 0xE0 {
            (2, 1)
        } else if b < 0xF0 {
            (3, 1)
        } else {
            (4, 2)
        };
        i += adv;
        units += u;
    }
    units
}

/// Indexed-iteration bound where recv is an `any` value (RFC
/// 20260704 S5). Iterable receivers answer their element /
/// code-unit count; everything else raises a catchable TypeError
/// (ES §7.4.2 GetIterator on a non-iterable) and answers 0. Since
/// S5+ this is no longer emitted directly by ssa-lower — it serves
/// as the per-step live bound inside `__torajs_any_iter_next`
/// (sibling iter_any module).
///
/// Strings iterate per UTF-16 code unit here (the element read is
/// `recv[i]`) — astral code points split into surrogate halves,
/// a documented deviation from the spec's per-code-point string
/// iteration tracked in the RFC (typed `for..of` over `string` has
/// its own per-cp path; this is the `any`-erased fallback).
///
/// # Safety
/// Cell receivers must be valid heap pointers matching their header
/// tag layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_iter_len(recv: AnyValue) -> i64 {
    if is_short_str(recv) {
        let len = short_str_len(recv) as usize;
        let bytes = short_str_bytes(recv);
        let payload = &bytes[..len];
        if payload.iter().all(|b| *b < 0x80) {
            return len as i64;
        }
        return utf16_units_of_utf8(payload);
    }
    if is_cell(recv) {
        let ptr = as_void_ptr(recv);
        unsafe {
            let tag = (ptr.cast::<u8>().add(4) as *const u16).read();
            if tag == Tag::Arr as u16 {
                return *(ptr.cast::<u8>().add(MIRROR_ARR_LEN_OFF) as *const u64) as i64;
            }
            if tag == Tag::Str as u16 {
                let flags = (ptr.cast::<u8>().add(6) as *const u16).read();
                return if flags & MIRROR_FLAG_SUBSTR_INLINE != 0 {
                    *(ptr.cast::<u8>().add(MIRROR_SUBSTR_LEN_OFF) as *const u64) as i64
                } else {
                    *(ptr.cast::<u8>().add(MIRROR_STR_LEN_OFF) as *const u32) as i64
                };
            }
        }
    }
    unsafe {
        __torajs_throw_type_error(c"value is not iterable".as_ptr());
    }
    0
}
