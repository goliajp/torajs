//! `Object.defineProperty(arr, key, desc)` — the Array exotic
//! DefineOwnProperty kernel (RFC 20260712-arr-exotic-define chunk B,
//! spec §10.4.2.1 + §10.1.6.3 data subset).
//!
//! Pre-fix `defineProperty(arr, "0", {...})` mis-routed to the
//! expando `arrprops_set` (values landed in the props dynobj where
//! index reads never look — a silent no-op), and the runtime Any
//! path handed the Arr cell straight to `dynobj_define`, which read
//! the Arr header (`len` @8) as a dynobj header (count/cap @8).
//!
//! ## Dispatch (`__torajs_arr_define`)
//! - `"length"` → §10.4.2.4 ArraySetLength: a present `[[Value]]`
//!   routes through `arr_set_length_any` (ToUint32 validate + real
//!   resize). The `writable: false` length lock is chunk D.
//! - canonical array index → [`define_index`].
//! - anything else → ordinary define on the expando props dynobj
//!   (full attribute flags — the plain-assign `arrprops_set` default
//!   stays for `arr.x = v`).
//!
//! ## Per-index attributes
//! The element value always lives in element storage; only the
//! *flags* of a defineProperty'd index live as a shadow entry
//! (canonical index key, value slot dead `undefined`) in the props
//! dynobj. `FLAG_ARR_EXOTIC_INDEX` on the header gates every reader
//! so arrays that never meet defineProperty pay one predictable
//! branch at most. Fresh defines complete absent flags to `false`
//! (spec §10.1.6.2), so a defineProperty'd index is almost always
//! exotic.

use core::ffi::c_void;

use torajs_rc::{FLAG_ARR_EXOTIC_INDEX, HeapHeader, Tag};

// The per-index attribute READERS live in the sibling (file-size
// rule); the re-export keeps every `crate::define::` consumer path
// unchanged.
pub(crate) use crate::define_index_flags::{__torajs_arr_index_flags, index_flags_with_key};

unsafe extern "C" {
    /// torajs-dynobj — ordinary define on the expando dynobj
    /// (validate + apply; lazily allocated via the slot).
    fn __torajs_dynobj_define(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    /// torajs-dynobj — ordinary define, boolean-answer flavor
    /// (rotation 267 刀 R5a): a §10.1.6.3 refusal answers 0 with no
    /// pending throw.
    fn __torajs_dynobj_define_soft(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    ) -> i64;
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_get_flags(dynobj: *const c_void, key: *const c_void) -> u64;
    /// torajs-dynobj — raw flags upsert for shadow entries (no
    /// §10.1.6.3 validation; this kernel already validated). A hit
    /// also revives a hole entry (value slot resets to live).
    fn __torajs_dynobj_set_entry_flags(obj_slot: *mut *mut c_void, key: *mut c_void, flags: u64);
    /// torajs-dynobj — HOLE sentinel probe (chunk C; the upsert
    /// lives in `define_hole.rs`).
    fn __torajs_dynobj_entry_is_hole(dynobj: *const c_void, key: *const c_void) -> i32;
    /// torajs-str — canonical index key mint for the flags probe.
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_value_drop_heap(p: *mut c_void);
}

// flags_byte layout mirror (torajs-dynobj::layout DEFINE_*): low 3
// bits = flag value, bits 3-5 = flag present, bit 6 = value present.
pub(crate) const F_WRITABLE: u64 = 1 << 0;
pub(crate) const F_ENUMERABLE: u64 = 1 << 1;
pub(crate) const F_CONFIGURABLE: u64 = 1 << 2;
pub(crate) const P_WRITABLE: u64 = 1 << 3;
pub(crate) const P_ENUMERABLE: u64 = 1 << 4;
pub(crate) const P_CONFIGURABLE: u64 = 1 << 5;
pub(crate) const P_VALUE: u64 = 1 << 6;
/// Accessor-face present bits (DEFINE_PRESENT_GET / _SET mirrors) —
/// route to the chunk-C accessor arm.
const P_GET: u64 = 1 << 7;
const P_SET: u64 = 1 << 8;
/// All three attribute bits — the implicit flags of a plain element.
pub(crate) const FLAGS_DEFAULT: u64 = F_WRITABLE | F_ENUMERABLE | F_CONFIGURABLE;
/// `arr_index_flags` RESULT bit (not a descriptor `flags_byte` bit):
/// the index was deleted — element storage stays dense but the index
/// is not an own property (RFC 20260713-defprop-tpd-cluster chunk C).
/// Every consumer treats a hole as absent: reads answer undefined,
/// has/gOPD answer absent, enumeration skips, writes re-create.
pub const F_HOLE: u64 = 1 << 3;

/// Key Str layout mirror — len u32 at +8, payload at +16.
const STR_LEN_OFF: usize = 8;
pub(crate) const STR_DATA_OFF: usize = 16;

/// `AnySlotTag` mirrors.
pub(crate) const ANY_HEAP: u64 = 4;
pub(crate) const ANY_UNDEF: u64 = 5;

/// A key's Str payload, or an empty slice when the key is a Symbol.
///
/// §6.1.7 lets a property key be either, and the two cells overlap
/// where it hurts: a Str keeps `len` at +8, a Symbol keeps its
/// description pointer there. Building the slice from that pointer
/// spans gigabytes of whatever follows the cell — the length-first
/// comparisons below happen not to read it, but the slice itself is
/// already outside what this crate is allowed to describe.
///
/// A symbol names neither `length` nor an array index, so the empty
/// slice routes it to the ordinary-key arm, which is where it belongs.
unsafe fn key_bytes<'a>(key: *const c_void) -> &'a [u8] {
    if unsafe { (*(key as *const HeapHeader)).type_tag } == Tag::Symbol as u16 {
        return &[];
    }
    let len = unsafe { key.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() };
    unsafe { core::slice::from_raw_parts(key.cast::<u8>().add(STR_DATA_OFF), len as usize) }
}

/// Canonical array-index parse — `arr_reflect.rs` (torajs-meta) twin.
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

/// Release a transferred `tag == ANY_HEAP` rc on a rejected define.
pub(crate) unsafe fn drop_owned(tag: u64, value: u64) {
    if tag == ANY_HEAP && value != 0 {
        unsafe { __torajs_value_drop_heap(value as *mut c_void) };
    }
}

/// Props-dynobj slot pointer (offset mirror of `crate::props`).
#[inline]
pub(crate) unsafe fn props_slot(arr: *mut c_void) -> *mut *mut c_void {
    unsafe { (arr as *mut u8).add(crate::layout::ARR_PROPS_OFF) as *mut *mut c_void }
}

/// Header flags word (u16 @6).
#[inline]
pub(crate) unsafe fn header_flags(arr: *const c_void) -> u16 {
    unsafe { (arr.cast::<u8>().add(6) as *const u16).read() }
}

/// Mint a pooled Str carrying the canonical decimal digits of `idx`
/// (the shadow-entry key shape). Caller owns the returned Str.
pub(crate) unsafe fn mint_index_key(idx: u64) -> *mut u8 {
    let mut buf = [0u8; 20];
    let mut n = buf.len();
    let mut v = idx;
    loop {
        n -= 1;
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let digits = &buf[n..];
    let key = unsafe { __torajs_str_alloc_pooled(digits.len() as u64) };
    unsafe { core::ptr::copy_nonoverlapping(digits.as_ptr(), key.add(STR_DATA_OFF), digits.len()) };
    key
}

/// §10.4.2.1 ArrayDefineOwnProperty entry — see module doc. `tag` /
/// `value` are honored only with `P_VALUE` set; an `ANY_HEAP` value
/// transfers one rc (consumed on store, released on rejection).
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer; `key` a live Str. Caller
/// must check for a pending throw after return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_define(
    arr: *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
) {
    unsafe { arr_define_impl(arr, key, tag, value, flags_byte, true) };
}

/// §28.1.2 Reflect.defineProperty flavor — identical
/// ArrayDefineOwnProperty walk, but a §10.1.6.3 refusal answers 0
/// with NO pending throw (the TypeError belongs to
/// Object.defineProperty's §20.1.2.4 caller, not to
/// [[DefineOwnProperty]] itself). A ToNumber throw / RangeError on
/// the `length` [[Value]] still records — those are conversions the
/// spec runs inside ArraySetLength, not refusals.
///
/// # Safety
/// Same contract as [`__torajs_arr_define`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_define_soft(
    arr: *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
) -> i64 {
    unsafe { arr_define_impl(arr, key, tag, value, flags_byte, false) }
}

/// Shared refusal answer — release the transferred [[Value]] stake,
/// record the TypeError only for the throwing flavor, answer 0.
pub(crate) unsafe fn refuse(
    throw_on_refusal: bool,
    msg: *const core::ffi::c_char,
    tag: u64,
    value: u64,
) -> i64 {
    unsafe { drop_owned(tag, value) };
    if throw_on_refusal {
        unsafe { __torajs_throw_type_error(msg) };
    }
    0
}

unsafe fn arr_define_impl(
    arr: *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
    throw_on_refusal: bool,
) -> i64 {
    let bytes = unsafe { key_bytes(key) };
    if bytes == b"length" {
        if flags_byte & (P_GET | P_SET) != 0 {
            // §10.4.2.4 — length is a data property; an accessor
            // redefine of a non-configurable property is a refusal.
            return unsafe {
                refuse(
                    throw_on_refusal,
                    c"Attempting to change configurable attribute of unconfigurable property."
                        .as_ptr(),
                    tag,
                    value,
                )
            };
        }
        return unsafe {
            crate::define_length::define_length(arr, tag, value, flags_byte, throw_on_refusal)
        };
    }
    if let Some(idx) = canonical_index(bytes) {
        if flags_byte & (P_GET | P_SET) != 0 {
            return unsafe {
                crate::define_accessor::define_index_accessor(
                    arr,
                    key,
                    idx,
                    tag,
                    value,
                    flags_byte,
                    throw_on_refusal,
                )
            };
        }
        return unsafe {
            crate::define_index::define_index(
                arr,
                key,
                idx,
                tag,
                value,
                flags_byte,
                throw_on_refusal,
            )
        };
    }
    // Ordinary key — full-flags define on the expando props dynobj
    // (lazily allocated; dynobj_define's null-obj guard would no-op).
    let slot = unsafe { props_slot(arr) };
    if unsafe { (*slot).is_null() } {
        unsafe { *slot = __torajs_dynobj_alloc() };
    }
    if throw_on_refusal {
        unsafe { __torajs_dynobj_define(slot, key, tag, value, flags_byte) };
        1
    } else {
        unsafe { __torajs_dynobj_define_soft(slot, key, tag, value, flags_byte) }
    }
}

/// Write the shadow flags entry + raise the header exotic bit.
pub(crate) unsafe fn store_shadow(arr: *mut c_void, key: *mut c_void, flags: u64) {
    let slot = unsafe { props_slot(arr) };
    if unsafe { (*slot).is_null() } {
        unsafe { *slot = __torajs_dynobj_alloc() };
    }
    unsafe { __torajs_dynobj_set_entry_flags(slot, key, flags) };
    let p = unsafe { (arr as *mut u8).add(6) as *mut u16 };
    unsafe { p.write(p.read() | FLAG_ARR_EXOTIC_INDEX) };
}
