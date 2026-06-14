//! W-N-b — `Object.getOwnPropertyNames(arr)` Arr-receiver path.
//! Spec §22.1.3.5: returns `["0", ..., "<len-1>", "length"]`. tora's
//! SSA-lower handles `Object.getOwnPropertyNames(struct-obj)` at
//! compile time (static field list) but Arr<T> requires a runtime
//! walk because len is dynamic. Builds an owned `Arr<Str>` cell via
//! `__torajs_arr_alloc(len + 1)` + N pushes of `__torajs_i64_to_str`
//! results + a final push of the `"length"` key. Caller receives a
//! +1-rc Arr<Str> ptr (returned through `Type::Arr<Str>` at the SSA
//! layer; LLVM ABI keeps it interchangeable with `Type::Ptr`).

use core::ffi::c_void;

unsafe extern "C" {
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    fn __torajs_i64_to_str(n: i64) -> *mut u8;
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_str_at(s: *const u8, i: i64) -> *mut u8;
}

#[inline]
unsafe fn alloc_str_literal(name: &[u8]) -> *mut u8 {
    let s = unsafe { __torajs_str_alloc_pooled(name.len() as u64) };
    if !name.is_empty() {
        unsafe { core::ptr::copy_nonoverlapping(name.as_ptr(), s.add(16), name.len()) };
    }
    s
}

/// Build the `Arr<Str>` name list for `Object.getOwnPropertyNames(arr)`.
/// SSA-lower pre-Loads `arr.len` from `ARR_LEN_OFF=8` so this helper
/// only needs the length to do its alloc-and-push loop.
///
/// # Safety
///
/// `extern "C"` ABI. Returned pointer owns a fresh `+1`-rc `Arr<Str>`
/// of length `len + 1`. The caller transfers that ownership into the
/// SSA value flow.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_index_strs(len: i64) -> *mut c_void {
    let mut out = unsafe { __torajs_arr_alloc((len as u64) + 1) };
    for i in 0..len {
        let s = unsafe { __torajs_i64_to_str(i) };
        out = unsafe { __torajs_arr_push(out, s as i64) };
    }
    let s_len = unsafe { alloc_str_literal(b"length") };
    out = unsafe { __torajs_arr_push(out, s_len as i64) };
    out as *mut c_void
}

/// W-N-d — `Object.getOwnPropertyNames(str)` Str-receiver path.
/// Same result shape as the Arr arm (`["0", ..., "<len-1>", "length"]`,
/// spec §22.1.5.2.4: string's own enumerable properties are the index
/// chars + the inherited-but-listed `length`). Thin wrapper that
/// reads the u32 length at `STR_LEN_OFF=8` then delegates.
///
/// # Safety
///
/// `str_ptr` must point at a valid Str heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_index_strs(str_ptr: *const c_void) -> *mut c_void {
    // SAFETY: STR_LEN_OFF=8 holds the live u32 length per torajs-str layout.
    let len = unsafe { (str_ptr.cast::<u8>().add(8) as *const u32).read() } as i64;
    unsafe { __torajs_arr_index_strs(len) }
}

/// W-O-2 — `Object.values(str)` per-character Str array. Spec
/// §22.1.5.2 + §20.1.2.20: ToObject on a primitive string materializes
/// the indexed-property view, whose values are the per-char fresh
/// Strs. Loops `__torajs_str_at(str_ptr, i)` to mint one fresh Str
/// per code unit (the same materialize path used by W-M-rest's
/// numeric-index descriptor — avoids the Substr view dynobj-store
/// round-trip trap that L3b had to fix separately for FLAG_SUBSTR_VIEW).
///
/// # Safety
///
/// `str_ptr` must point at a valid Str heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_to_char_arr(str_ptr: *const c_void) -> *mut c_void {
    // SAFETY: STR_LEN_OFF=8 holds the live u32 length per torajs-str layout.
    let len = unsafe { (str_ptr.cast::<u8>().add(8) as *const u32).read() } as i64;
    let mut out = unsafe { __torajs_arr_alloc(len.max(0) as u64) };
    for i in 0..len {
        let ch = unsafe { __torajs_str_at(str_ptr.cast::<u8>(), i) };
        out = unsafe { __torajs_arr_push(out, ch as i64) };
    }
    out as *mut c_void
}
