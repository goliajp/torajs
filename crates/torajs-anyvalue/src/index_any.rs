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
//! - `Tag::DynObj` (L3b #3, chunk 527) → the numeric key stringifies
//!   to its decimal form (`o[42]` ≡ `o["42"]` per ES §7.1.19
//!   ToPropertyKey) and probes own properties; an accessor entry's
//!   getter runs (getter-as-value). Absence answers `undefined`.
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
    /// torajs-dynobj — keyed store; resize relocates through the
    /// slot (signature mirrors the crate's method_call_mapset
    /// declaration).
    fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
    /// torajs-dynobj — run an accessor entry's getter; the answer is
    /// an owned AnyValue per the boxed-value convention.
    fn __torajs_accessor_invoke_getter(pair: *const c_void) -> u64;
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
        return unsafe { dynobj_index_get(ptr, idx) };
    }
    // Chunk 744 — struct cell: ToPropertyKey the index and probe the
    // class layout (`{0:"a"} as any` boxes as an anon-struct whose
    // field NAME is the decimal spelling; pre-fix `e[0]` answered a
    // silent undefined).
    if tag == Tag::Obj as u16 {
        return unsafe { struct_index_get(ptr, idx) };
    }
    VALUE_UNDEFINED
}

/// `Tag::Obj` get arm — decimal-stringify the key and probe the class
/// layout through [`crate::member_get::struct_field_pair`]; the probe
/// is a borrow, the returned box owns its own reference (the
/// `dynobj_index_get` shape).
unsafe fn struct_index_get(obj: *mut c_void, idx: i64) -> AnyValue {
    let mut buf = [0u8; 20];
    let (start, len) = i64_dec(&mut buf, idx);
    unsafe {
        let key = __torajs_str_alloc(buf[start..].as_ptr(), len as i64);
        let pair = crate::member_get::struct_field_pair(obj, key as *const c_void);
        __torajs_str_drop(key as *mut c_void);
        let Some((ftag, fval)) = pair else {
            return VALUE_UNDEFINED;
        };
        crate::payload_rc_inc(ftag as i64, fval as i64);
        crate::nanbox_encode::__torajs_anyv_box_from_pair(ftag as i64, fval as i64)
    }
}

/// Stack decimal formatter for the ToPropertyKey stringification
/// (in-house — metal-level crates take no external deps). Fills the
/// 20-byte buffer backward; answers `(start, len)` into it. 20 bytes
/// covers i64::MIN (`-9223372036854775808`).
fn i64_dec(buf: &mut [u8; 20], v: i64) -> (usize, usize) {
    let neg = v < 0;
    // Two's-complement magnitude — safe for i64::MIN.
    let mut m = (v as i128).unsigned_abs() as u64;
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (m % 10) as u8;
        m /= 10;
        if m == 0 {
            break;
        }
    }
    if neg {
        i -= 1;
        buf[i] = b'-';
    }
    (i, buf.len() - i)
}

/// `Tag::DynObj` get arm — decimal-stringify the key and probe own
/// properties (same borrow-then-own shape as the `"length"` probe
/// below); an accessor entry's getter answers its owned result.
unsafe fn dynobj_index_get(obj: *mut c_void, idx: i64) -> AnyValue {
    let mut buf = [0u8; 20];
    let (start, len) = i64_dec(&mut buf, idx);
    unsafe {
        let key = __torajs_str_alloc(buf[start..].as_ptr(), len as i64);
        let dtag = __torajs_dynobj_get_tag(obj, key as *const c_void);
        let dval = __torajs_dynobj_get_value(obj, key as *const c_void);
        __torajs_str_drop(key as *mut c_void);
        // Accessor sentinel (tag 6) — run the getter; its answer is
        // owned per the boxed-value convention.
        if dtag == INDEX_ANY_ACCESSOR_TAG {
            return __torajs_accessor_invoke_getter(dval as *const c_void);
        }
        // The probe pair is a borrow — the returned box owns its own
        // reference.
        crate::payload_rc_inc(dtag as i64, dval as i64);
        crate::nanbox_encode::__torajs_anyv_box_from_pair(dtag as i64, dval as i64)
    }
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
/// - `Tag::DynObj` (L3b #3, chunk 527) → the numeric key
///   stringifies to its decimal form and stores through
///   `__torajs_dynobj_set` (the pair transfers into the bucket); a
///   resize-relocated block writes the fresh cell back through
///   `recv_slot` (NULL for non-Ident receivers — same canonical-slot
///   shape as the member-set gate).
/// - any other heap tag → explicit TypeError.
///
/// # Safety
/// Cell receivers must be valid heap pointers matching their header
/// tag layout; a `tag == 4` `value` must be 0 or a valid owned heap
/// pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_index_set(
    recv: AnyValue,
    idx: i64,
    tag: u64,
    value: u64,
    recv_slot: *mut u64,
) {
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
    if hdr_tag == Tag::DynObj as u16 {
        let mut buf = [0u8; 20];
        let (start, len) = i64_dec(&mut buf, idx);
        unsafe {
            let key = __torajs_str_alloc(buf[start..].as_ptr(), len as i64);
            let mut obj = ptr;
            __torajs_dynobj_set(&mut obj, key as *mut c_void, tag, value);
            __torajs_str_drop(key as *mut c_void);
            if obj != ptr && !recv_slot.is_null() {
                // Resize relocated the block — the NaN-box cell
                // encoding is the pointer bits; transfer, no rc
                // traffic (same identity, moved storage).
                *recv_slot = crate::nanbox_encode::__torajs_anyv_box_from_pair(4, obj as i64);
            }
        }
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
pub(crate) const MIRROR_ARR_LEN_OFF: usize = 8;
// Accessor-entry sentinel in the dynobj probe's tag channel — mirror
// of torajs-core `ssa_lower_accessor.rs::ANY_ACCESSOR_TAG`.
const INDEX_ANY_ACCESSOR_TAG: u64 = 6;
pub(crate) const MIRROR_STR_LEN_OFF: usize = 8;
pub(crate) const MIRROR_SUBSTR_LEN_OFF: usize = 8;
pub(crate) const MIRROR_FLAG_SUBSTR_INLINE: u16 = 1 << 0;

/// Count UTF-16 code units of a UTF-8 byte payload (≤ 5 bytes for a
/// ShortStr, so a simple lead-byte walk suffices; 4-byte sequences
/// decode to astral cps = 2 units).
pub(crate) fn utf16_units_of_utf8(payload: &[u8]) -> i64 {
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
