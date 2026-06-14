//! W-M-rest — `Object.getOwnPropertyDescriptor(str, "<numeric>")` —
//! a string's numeric-indexed own property. Spec ES §22.1.5.2: each
//! code-unit `s[i]` is a `{value: char, writable: false, enumerable:
//! true, configurable: false}` data property (note `enumerable: true`,
//! unlike the `length` descriptor's all-false flags). Out-of-range /
//! negative / non-canonical indices return `undefined`.
//!
//! W-M handled the static `"length"` key; this helper handles the
//! dynamic numeric-index path. SSA-lower pre-Loads no length (the
//! bound check happens inside the helper) and passes `(str_ptr,
//! idx: i64)` so the cost on the cold path is one helper call.

use core::ffi::c_void;

unsafe extern "C" {
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_str_drop(s: *mut u8);
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(dst: *mut *mut c_void, key: *const u8, tag: u64, value: u64);
    fn __torajs_str_at(s: *const u8, i: i64) -> *mut u8;
}

const ANY_BOOL: u64 = 1;
const ANY_HEAP: u64 = 4;
const VALUE_UNDEFINED_IMM: u64 = 0x0A;

#[inline]
unsafe fn alloc_str_literal(name: &[u8]) -> *mut u8 {
    let s = unsafe { __torajs_str_alloc_pooled(name.len() as u64) };
    if !name.is_empty() {
        unsafe { core::ptr::copy_nonoverlapping(name.as_ptr(), s.add(16), name.len()) };
    }
    s
}

/// Build the 4-key descriptor for `s[idx]`, or return
/// [`VALUE_UNDEFINED_IMM`] if `idx` is negative or `>= s.length`.
/// `value` owns the +1-rc returned by `__torajs_str_char_at` (which
/// allocs a fresh single-code-unit Substr); the descriptor cell takes
/// ownership of that share when the entry is set.
///
/// # Safety
///
/// `str_ptr` must point at a valid Str heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_str_index_descriptor(
    str_ptr: *const c_void,
    idx: i64,
) -> u64 {
    // SAFETY: STR_LEN_OFF=8 holds the live u32 length per torajs-str layout.
    let len = unsafe { (str_ptr.cast::<u8>().add(8) as *const u32).read() } as i64;
    if idx < 0 || idx >= len {
        return VALUE_UNDEFINED_IMM;
    }
    // Use `str_at` (alloc_str_slice path → fresh Str) instead of
    // `str_char_at` (Substr view). Substr values in dynobj `value`
    // slots don't round-trip cleanly through the print_anyv dispatch
    // (handoff L3b: dynobj-stored Substr renders as garbage code
    // units). The fresh Str copies one code unit + is round-trip safe.
    let ch = unsafe { __torajs_str_at(str_ptr.cast::<u8>(), idx) };
    let mut desc = unsafe { __torajs_dynobj_alloc() };
    let entries: [(&[u8], u64, u64); 4] = [
        (b"value", ANY_HEAP, ch as u64),
        (b"writable", ANY_BOOL, 0),
        (b"enumerable", ANY_BOOL, 1),
        (b"configurable", ANY_BOOL, 0),
    ];
    for &(name, t, val) in entries.iter() {
        let k = unsafe { alloc_str_literal(name) };
        unsafe { __torajs_dynobj_set(&mut desc, k, t, val) };
        unsafe { __torajs_str_drop(k) };
    }
    desc as u64
}
