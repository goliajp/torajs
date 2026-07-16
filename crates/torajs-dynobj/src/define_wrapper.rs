//! Primitive-wrapper receiver arm of [`crate::define::define_apply`]
//! — pre-fix a wrapper cell fell through to the dynobj lane, whose
//! `probe` / `entries` walk read the wrapper's `[header:8][value:8]
//! [props:8]` layout as a dynobj header (count/cap @8): silent
//! corruption, `Object.defineProperty(new String("ab"), ...)`
//! SIGSEGV'd on the first call.
//!
//! Two lanes:
//!
//! - **StringWrapper inherent keys** (`"length"` / a canonical index
//!   `< len`): ES §10.4.3.2 String exotic [[DefineOwnProperty]] over
//!   the fixed §22.1.4 attributes (`length` w:0/e:0/c:0, index
//!   w:0/e:1/c:0, both non-configurable). The §10.1.6.3 validation
//!   collapses to: any flag upgrade or value change → TypeError,
//!   fully-compatible descriptor → no-op.
//! - **everything else** (Number / Boolean wrappers included) —
//!   OrdinaryDefineOwnProperty on the `+16` lazy expando dynobj
//!   (mirror of member_set's wrapper [[Set]] lane), gated by the
//!   wrapper header's non-extensible bit for fresh keys.

use core::ffi::c_void;

use crate::layout::{
    ANY_HEAP, DEFINE_FLAG_CONFIGURABLE, DEFINE_FLAG_ENUMERABLE, DEFINE_FLAG_WRITABLE,
    DEFINE_PRESENT_CONFIGURABLE, DEFINE_PRESENT_ENUMERABLE, DEFINE_PRESENT_VALUE,
    DEFINE_PRESENT_WRITABLE, DYNOBJ_HDR_FLAG_NON_EXTENSIBLE,
};
use crate::probe::probe;

/// `torajs_rc::Tag::{NumberWrapper,StringWrapper,BooleanWrapper}`
/// mirrors — the `define_apply` dispatch keys on these.
pub(crate) const TAG_NUMBER_WRAPPER: u16 = 21;
pub(crate) const TAG_STRING_WRAPPER: u16 = 22;
pub(crate) const TAG_BOOLEAN_WRAPPER: u16 = 23;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const u8);
    fn __torajs_value_drop_heap(child: *mut c_void);
    fn __torajs_dynobj_alloc() -> *mut c_void;
    /// torajs-anyvalue — NaN-box AnyValue pair encoder.
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    /// torajs-anyvalue — `===` over two NaN-box AnyValues (string
    /// content compare across Str / Substr / ShortStr shapes; the
    /// SameValue delta — NaN / ±0 — cannot arise for a char cell or
    /// a u32 length).
    fn __torajs_anyv_strict_eq(a: u64, b: u64) -> bool;
    /// torajs-str — fresh owned single-code-unit Str at `i`.
    fn __torajs_str_at(s: *const u8, i: i64) -> *mut u8;
}

/// Wrapper `[[StringData]]` inner Str-cell slot / lazy expando slot
/// (`[header:8][value:8][props:8]`, RFC 20260716 刀 4/5).
const WRAPPER_INNER_OFF: usize = 8;
const WRAPPER_PROPS_OFF: usize = 16;

/// Canonical numeric index per §7.1.22 (mirror of
/// `torajs-meta::arr_reflect::canonical_index`).
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

/// §10.1.6.3 ValidateAndApplyPropertyDescriptor against a fixed
/// non-configurable data slot (`cur_anyv` borrowed, `cur_enumerable`
/// per §22.1.4). Any upgrade / mismatch throws; a fully-compatible
/// descriptor is a spec no-op. Either way the desc's owned value
/// stake is released (the entry never adopts it).
unsafe fn validate_immutable_slot(
    tag: u64,
    value: u64,
    flags_byte: u64,
    cur_anyv: u64,
    cur_enumerable: bool,
) {
    // A data → accessor conversion on a non-configurable slot is a
    // §10.1.6.3 step-4 reject — the accessor path hands the minted
    // AccessorPair through the same (ANY_HEAP, ptr) shape, detected
    // by the pointee's type_tag like everywhere else.
    let is_accessor_desc = tag == ANY_HEAP
        && value != 0
        && unsafe { ((value as *const u8).add(4) as *const u16).read() }
            == crate::accessor::TAG_ACCESSOR_PAIR;
    let incompatible = is_accessor_desc
        || (flags_byte & DEFINE_PRESENT_CONFIGURABLE != 0
            && flags_byte & DEFINE_FLAG_CONFIGURABLE != 0)
        || (flags_byte & DEFINE_PRESENT_WRITABLE != 0 && flags_byte & DEFINE_FLAG_WRITABLE != 0)
        || (flags_byte & DEFINE_PRESENT_ENUMERABLE != 0
            && (flags_byte & DEFINE_FLAG_ENUMERABLE != 0) != cur_enumerable)
        || (flags_byte & DEFINE_PRESENT_VALUE != 0
            && unsafe {
                !__torajs_anyv_strict_eq(
                    __torajs_anyv_box_from_pair(tag as i64, value as i64),
                    cur_anyv,
                )
            });
    // The desc's value arrives with one owned stake (define_apply's
    // consume contract); neither leg stores it.
    if tag == ANY_HEAP && value != 0 {
        unsafe { __torajs_value_drop_heap(value as *mut c_void) };
    }
    if incompatible {
        unsafe {
            __torajs_throw_type_error(
                c"Attempting to change value of a readonly property.".as_ptr() as *const u8,
            );
        }
    }
}

/// See module doc.
///
/// # Safety
/// `obj` is a live wrapper cell (Number / String / Boolean tag,
/// checked by the `define_apply` dispatch); `key` is a live Str.
/// Caller must check for pending throw after return.
pub(crate) unsafe fn wrapper_define(
    obj: *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
) {
    let htag = unsafe { (obj.cast::<u8>().add(4) as *const u16).read() };
    if htag == TAG_STRING_WRAPPER {
        let inner =
            unsafe { (obj.cast::<u8>().add(WRAPPER_INNER_OFF) as *const u64).read() } as *const u8;
        // Tag::Str layout — code-unit count u32 @ +8; NULL inner is
        // the `new String()` empty-string sentinel.
        let len = if inner.is_null() {
            0u64
        } else {
            let units = unsafe { (inner.add(8) as *const u32).read() };
            units as u64
        };
        let key_len = unsafe { (key.cast::<u8>().add(8) as *const u32).read() } as usize;
        let key_bytes =
            unsafe { core::slice::from_raw_parts(key.cast::<u8>().add(16) as *const u8, key_len) };
        if key_bytes == b"length" {
            let cur = unsafe { __torajs_anyv_box_from_pair(2, len as i64) };
            unsafe { validate_immutable_slot(tag, value, flags_byte, cur, false) };
            return;
        }
        if let Some(idx) = canonical_index(key_bytes) {
            if idx < len {
                let ch = unsafe { __torajs_str_at(inner, idx as i64) };
                let cur = unsafe { __torajs_anyv_box_from_pair(ANY_HEAP as i64, ch as i64) };
                unsafe { validate_immutable_slot(tag, value, flags_byte, cur, true) };
                unsafe { __torajs_value_drop_heap(ch as *mut c_void) };
                return;
            }
        }
    }
    // Expando lane — fresh keys on a non-extensible wrapper reject
    // (the bit lives on the wrapper header, not the lazily-allocated
    // expando; mirror of member_set's wrapper [[Set]] gate).
    let props_slot = unsafe { obj.cast::<u8>().add(WRAPPER_PROPS_OFF) } as *mut u64;
    let mut props = unsafe { *props_slot } as *mut c_void;
    let wrapper_flags = unsafe { (obj.cast::<u8>().add(6) as *const u16).read() };
    if wrapper_flags & DYNOBJ_HDR_FLAG_NON_EXTENSIBLE != 0 {
        let present = !props.is_null() && unsafe { probe(props, key as *const c_void) }.found;
        if !present {
            unsafe {
                __torajs_throw_type_error(
                    c"Attempting to define property on object that is not extensible.".as_ptr()
                        as *const u8,
                );
            }
            return;
        }
    }
    if props.is_null() {
        props = unsafe { __torajs_dynobj_alloc() };
    }
    unsafe { crate::define::define_apply(&mut props, key, tag, value, flags_byte) };
    unsafe { *props_slot = props as u64 };
}
