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

use torajs_rc::{AnySlotTag, Tag};

use crate::nanbox::AnyValue;

unsafe extern "C" {
    fn __torajs_arraybuffer_byte_length(av: AnyValue) -> i64;
    fn __torajs_arraybuffer_max_byte_length(av: AnyValue) -> i64;
    fn __torajs_arraybuffer_resizable(av: AnyValue) -> i64;
    fn __torajs_arraybuffer_detached(av: AnyValue) -> i64;
    fn __torajs_typedarray_length(av: AnyValue) -> i64;
    fn __torajs_typedarray_byte_length(av: AnyValue) -> i64;
    fn __torajs_typedarray_byte_offset(av: AnyValue) -> i64;
    fn __torajs_typedarray_buffer(av: AnyValue) -> AnyValue;
    fn __torajs_typedarray_bytes_per_element(av: AnyValue) -> i64;
    fn __torajs_dataview_byte_length(av: AnyValue) -> i64;
    fn __torajs_dataview_byte_offset(av: AnyValue) -> i64;
    fn __torajs_dataview_buffer(av: AnyValue) -> AnyValue;
}

/// The view's current element count (re-derived each call — a
/// tracking view over a resizable buffer changes length without
/// being touched). Detached answers 0.
///
/// # Safety
/// `recv` is a live TypedArray AnyValue.
pub(crate) unsafe fn typedarray_len(recv: AnyValue) -> i64 {
    unsafe { __torajs_typedarray_length(recv) }
}

/// §7.1.21 CanonicalNumericIndexString over a Str-cell key —
/// `Some(n)` when the spelling is `"-0"` or round-trips through
/// ToString(ToNumber(key)). The buffer family needs the full
/// grammar, not just array indices: §10.4.5 routes EVERY canonical
/// numeric spelling ("-0", "1.5", "NaN") to the element face, where
/// an invalid one still coerces the value and then drops the store.
///
/// # Safety
/// `key` is NULL or a live Str cell.
pub(crate) unsafe fn canonical_numeric_key(key: *const c_void) -> Option<f64> {
    if key.is_null() {
        return None;
    }
    let k = unsafe { crate::prop_has::key_bytes(key) };
    let s = k.as_bytes();
    if s.is_empty() || s.len() > 32 {
        return None;
    }
    // Integer fast path — the overwhelmingly common spelling, no
    // allocation. Leading zeros are non-canonical ("01" != "1").
    if s.iter().all(u8::is_ascii_digit) && !(s.len() > 1 && s[0] == b'0') {
        let mut v: u64 = 0;
        for &b in s {
            v = v.checked_mul(10)?.checked_add((b - b'0') as u64)?;
        }
        // Beyond 2^53 the decimal spelling stops round-tripping
        // exactly; fall to the printed comparison below.
        if v < (1u64 << 53) {
            return Some(v as f64);
        }
    }
    if s == b"-0" {
        return Some(-0.0);
    }
    if s == b"NaN" {
        return Some(f64::NAN);
    }
    if s == b"Infinity" {
        return Some(f64::INFINITY);
    }
    if s == b"-Infinity" {
        return Some(f64::NEG_INFINITY);
    }
    // General form — parse, print with the JS ToString kernel, and
    // demand the exact spelling back.
    let txt = core::str::from_utf8(s).ok()?;
    let n: f64 = txt.parse().ok()?;
    if !n.is_finite() {
        // "1e999" parses to inf but does not spell "Infinity".
        return None;
    }
    unsafe {
        let printed = crate::__torajs_f64_to_str(n);
        let eq = crate::prop_has::key_is(printed as *const c_void, s);
        crate::__torajs_str_drop(printed);
        if eq { Some(n) } else { None }
    }
}

/// The ABI face of [`canonical_numeric_key`] for torajs-dynobj's
/// [[DefineOwnProperty]] arm: 1 = numeric (with `out` written).
///
/// # Safety
/// `key` is NULL or a live Str cell; `out` is a valid slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_buffer_canonical_numeric_index(
    key: *const c_void,
    out: *mut f64,
) -> i64 {
    match unsafe { canonical_numeric_key(key) } {
        Some(n) => {
            unsafe { *out = n };
            1
        }
        None => 0,
    }
}

/// Does `tag` name a cell whose accessors this module answers? One
/// question asked once, so the two probe channels each keep a single
/// match arm instead of one per buffer-family tag.
#[inline]
pub(crate) fn is_buffer_family(tag: u16) -> bool {
    tag == Tag::ArrayBuffer as u16 || tag == Tag::TypedArray as u16 || tag == Tag::DataView as u16
}

/// The accessor pair for whichever buffer-family cell `recv` is.
///
/// # Safety
/// `recv` is a live ArrayBuffer or TypedArray AnyValue; `key` is
/// NULL or a live Str cell.
pub(crate) unsafe fn buffer_family_prop(
    recv: AnyValue,
    key: *const c_void,
    tag: u16,
) -> Option<(u64, u64)> {
    if tag == Tag::TypedArray as u16 {
        return unsafe { typedarray_prop(recv, key) };
    }
    if tag == Tag::DataView as u16 {
        return unsafe { dataview_prop(recv, key) };
    }
    unsafe { arraybuffer_prop(recv, key) }
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

/// The §23.2.3 accessors a typed array answers on the probe pair.
/// `buffer` is the one that hands back a cell, and the probe pair is
/// a BORROW convention — so the reference the getter takes is given
/// straight back and the pair carries the cell unowned, exactly like
/// every other heap answer on this channel.
///
/// # Safety
/// `recv` is a live TypedArray AnyValue; `key` is NULL or a live Str
/// cell.
pub(crate) unsafe fn typedarray_prop(recv: AnyValue, key: *const c_void) -> Option<(u64, u64)> {
    unsafe {
        if crate::prop_has::key_is(key, b"length") {
            return Some(num(__torajs_typedarray_length(recv)));
        }
        if crate::prop_has::key_is(key, b"byteLength") {
            return Some(num(__torajs_typedarray_byte_length(recv)));
        }
        if crate::prop_has::key_is(key, b"byteOffset") {
            return Some(num(__torajs_typedarray_byte_offset(recv)));
        }
        if crate::prop_has::key_is(key, b"BYTES_PER_ELEMENT") {
            return Some(num(__torajs_typedarray_bytes_per_element(recv)));
        }
        if crate::prop_has::key_is(key, b"buffer") {
            let owned = __torajs_typedarray_buffer(recv);
            crate::__torajs_anyv_rc_dec(owned);
            return Some((
                AnySlotTag::Heap as u64,
                crate::nanbox::as_void_ptr(owned) as u64,
            ));
        }
        None
    }
}

/// The §25.3.4 accessors a DataView answers on the probe pair.
/// `byteLength` / `byteOffset` throw a TypeError over a detached
/// buffer or an out-of-bounds view (the kernels own that — unlike
/// the typed-array getters, which answer 0 there); `buffer` answers
/// regardless, same borrow shape as the typed-array arm's.
///
/// # Safety
/// `recv` is a live DataView AnyValue; `key` is NULL or a live Str
/// cell.
pub(crate) unsafe fn dataview_prop(recv: AnyValue, key: *const c_void) -> Option<(u64, u64)> {
    unsafe {
        if crate::prop_has::key_is(key, b"byteLength") {
            return Some(num(__torajs_dataview_byte_length(recv)));
        }
        if crate::prop_has::key_is(key, b"byteOffset") {
            return Some(num(__torajs_dataview_byte_offset(recv)));
        }
        if crate::prop_has::key_is(key, b"buffer") {
            let owned = __torajs_dataview_buffer(recv);
            crate::__torajs_anyv_rc_dec(owned);
            return Some((
                AnySlotTag::Heap as u64,
                crate::nanbox::as_void_ptr(owned) as u64,
            ));
        }
        None
    }
}

/// §7.3.12 HasProperty's prototype half for a buffer-family
/// receiver: does the family's PROTOTYPE own `key`? Name-level only
/// — `in` never invokes a getter, so this must not touch the
/// accessor kernels (an out-of-bounds DataView still answers true
/// for "byteLength" without throwing).
///
/// # Safety
/// `key` is NULL or a live Str cell.
pub(crate) unsafe fn buffer_proto_key(recv: *const c_void, tag: u16, key: *const c_void) -> bool {
    let names: &[&[u8]] = if tag == Tag::TypedArray as u16 {
        &[
            b"length",
            b"byteLength",
            b"byteOffset",
            b"buffer",
            b"BYTES_PER_ELEMENT",
        ]
    } else if tag == Tag::DataView as u16 {
        &[b"byteLength", b"byteOffset", b"buffer"]
    } else {
        &[b"byteLength", b"maxByteLength", b"resizable", b"detached"]
    };
    for n in names {
        if unsafe { crate::prop_has::key_is(key, n) } {
            return true;
        }
    }
    // Interned prototype methods — the same per-tag support tables
    // the method dispatch resolves with.
    if key.is_null() {
        return false;
    }
    let k = unsafe { crate::prop_has::key_bytes(key) };
    let Ok(name) = core::str::from_utf8(k.as_bytes()) else {
        return false;
    };
    let mid = torajs_rc::any_method_intern::any_method_id(name);
    if mid == torajs_rc::ANY_METHOD_UNKNOWN {
        return false;
    }
    if tag == Tag::TypedArray as u16 {
        unsafe { crate::method_support::typedarray_supports_on(recv, mid) }
    } else if tag == Tag::ArrayBuffer as u16 {
        crate::method_support::arraybuffer_supports(mid)
    } else {
        crate::method_support::dataview_supports(mid)
    }
}

/// [`buffer_proto_key`] at the C edge — torajs-rc's `in` kernel
/// reaches it link-time (that crate keeps zero Cargo deps).
///
/// # Safety
/// `key` is NULL or a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_buffer_family_proto_key(
    recv: *const c_void,
    tag: i64,
    key: *const c_void,
) -> i64 {
    unsafe { buffer_proto_key(recv, tag as u16, key) as i64 }
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
