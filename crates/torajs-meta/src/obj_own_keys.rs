//! RC-4 F1c — runtime chooser for `Object.keys` /
//! `Object.getOwnPropertyNames` / `Reflect.ownKeys` on struct-typed
//! receivers.
//!
//! `Object.defineProperty` converts a struct receiver to a DynObj and
//! rebinds the binding (`emit_any_dynobj_writeback`), so the
//! compile-time field list the SSA arm emits can be stale — a
//! runtime-defined property (test262 gOPN accessor family) was
//! invisible to reflection. The SSA arm still builds the static list
//! (correct for a plain struct cell, zero-cost reflection), then
//! routes through [`__torajs_obj_own_keys`]:
//!
//! - receiver is a DynObj cell → drop the static list and build the
//!   key array from the live entry walk in ES §10.1.11.1 order
//!   (array-index keys ascending, then insertion order).
//!   `include_nonenum = 0` (`Object.keys`) filters enumerable-only;
//!   `1` (`getOwnPropertyNames` / `ownKeys`) includes every key.
//! - anything else → return the static list as-is.

use core::ffi::c_void;

unsafe extern "C" {
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    fn __torajs_rc_inc(p: *mut c_void);
    /// `runtime_str.c` universal-drop dispatcher (settles the unused
    /// static list on the DynObj path).
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-dynobj iteration surface — keys are BORROWED.
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    fn __torajs_dynobj_iter_order(obj: *const c_void, out: *mut u64, cap: u64) -> u64;
    fn __torajs_dynobj_iter_flags(obj: *const c_void, i: u64) -> u64;
}

/// `HeapHeader::type_tag` mirror of `torajs_rc::Tag::DynObj` (locked
/// there); header field lives at byte offset 4.
const TAG_DYNOBJ: u16 = 14;
const HDR_TYPE_TAG_OFF: usize = 4;

/// `torajs_dynobj::layout::BUCKET_FLAG_ENUMERABLE` mirror (bit 1).
const FLAG_ENUMERABLE: u64 = 1 << 1;

/// Runtime chooser — see module doc. Returns a +1-rc `Arr<Str>`.
///
/// # Safety
/// `obj` is null or a live heap ptr with a universal header;
/// `static_names` is an owned +1 `Arr<Str>` this call consumes-or-
/// returns.
/// `true` iff the heap cell's `type_tag` is DynObj.
#[inline]
unsafe fn is_dynobj(obj: *const c_void) -> bool {
    unsafe { *((obj as *const u8).add(HDR_TYPE_TAG_OFF) as *const u16) == TAG_DYNOBJ }
}

/// Build the key `Arr<Str>` from a live DynObj walk in ES
/// §10.1.11.1 order. `include_nonenum = 0` filters enumerable-only.
unsafe fn dynobj_keys_walk(obj: *const c_void, include_nonenum: i64) -> *mut c_void {
    let len = unsafe { __torajs_dynobj_iter_len(obj) };
    let mut order = vec![0u64; len as usize];
    let n = unsafe { __torajs_dynobj_iter_order(obj, order.as_mut_ptr(), len) };
    let mut arr = unsafe { __torajs_arr_alloc(n) };
    for &i in order.iter().take(n as usize) {
        if include_nonenum == 0 {
            let flags = unsafe { __torajs_dynobj_iter_flags(obj, i) };
            if flags & FLAG_ENUMERABLE == 0 {
                continue;
            }
        }
        let key = unsafe { __torajs_dynobj_iter_key(obj, i) };
        if key.is_null() {
            continue;
        }
        // Borrowed key → the array slot takes its own share.
        unsafe { __torajs_rc_inc(key) };
        arr = unsafe { __torajs_arr_push(arr, key as i64) };
    }
    arr as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_own_keys(
    obj: *const c_void,
    static_names: *mut c_void,
    include_nonenum: i64,
) -> *mut c_void {
    if obj.is_null() || !unsafe { is_dynobj(obj) } {
        return static_names;
    }
    unsafe { __torajs_value_drop_heap(static_names) };
    unsafe { dynobj_keys_walk(obj, include_nonenum) }
}

/// `Object.keys` / `getOwnPropertyNames` / `Reflect.ownKeys` arm for
/// an `any`-typed receiver: a DynObj cell walks its live entries;
/// everything else delegates to the struct arm (which throws the
/// loud non-struct TypeError for non-struct cells — caller runs a
/// throw-check).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_own_keys(v: u64, include_nonenum: i64) -> *mut c_void {
    // Cell-imm check mirrors `struct_enum::is_cell_imm` — the tag
    // read below is only sound for a real heap ptr bit pattern.
    let top16_zero = v & 0xFFFF_0000_0000_0000 == 0;
    let not_sentinel = v & 0x2 == 0;
    if top16_zero && not_sentinel && v != 0 && unsafe { is_dynobj(v as *const c_void) } {
        return unsafe { dynobj_keys_walk(v as *const c_void, include_nonenum) };
    }
    unsafe { crate::struct_enum::__torajs_anyv_struct_keys(v) }
}
