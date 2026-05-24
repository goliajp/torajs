//! RegExp lifecycle externs — drop / get_source / lastIndex
//! get-set. Port of `runtime_regex.c` L1519-1552, L2130-2140.

use core::ffi::c_void;

use super::{
    __torajs_rc_dec, __torajs_str_alloc_pooled, RegExp, STR_HDR_SIZE, as_regex, as_regex_mut,
};

/// # Safety
///
/// `re_ptr` must be a pointer previously returned by
/// `__torajs_regex_compile`, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_drop(re_ptr: *mut c_void) {
    if re_ptr.is_null() {
        return;
    }
    // Refcount decrement — returns 0 when the last ref dropped (per
    // torajs-rc contract; matches the C port's `if
    // (!__torajs_rc_dec(re_ptr)) return;`).
    if unsafe { __torajs_rc_dec(re_ptr) } == 0 {
        return;
    }
    // Last ref — reclaim the Box and let Rust recursively Drop the
    // Program (Vec<Inst> + Vec<CharClass> + Vec<Box<Program>>),
    // src_bytes (Vec<u8>), capture_names (Vec<Vec<u8>>).
    unsafe {
        let _ = Box::from_raw(re_ptr as *mut RegExp);
    }
}

/// `re.source` — returns the original pattern bytes as a fresh
/// pooled `Str`. NULL receiver returns `""`. Port of
/// `__torajs_regex_get_source`.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`. Returned pointer is a
/// pool-Str with rc=1; caller takes ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_get_source(re_ptr: *const c_void) -> *mut c_void {
    if re_ptr.is_null() {
        return unsafe { __torajs_str_alloc_pooled(0) as *mut c_void };
    }
    let re = unsafe { as_regex(re_ptr) };
    let len = re.src_bytes.len() as u64;
    let s = unsafe { __torajs_str_alloc_pooled(len) };
    if len > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(
                re.src_bytes.as_ptr(),
                s.add(STR_HDR_SIZE),
                len as usize,
            );
        }
    }
    s as *mut c_void
}

/// `re.lastIndex` getter. Port of `__torajs_regex_get_last_index`.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_get_last_index(re_ptr: *const c_void) -> i64 {
    if re_ptr.is_null() {
        return 0;
    }
    unsafe { as_regex(re_ptr) }.last_index
}

/// `re.lastIndex = idx` setter. Port of
/// `__torajs_regex_set_last_index`.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_set_last_index(re_ptr: *mut c_void, idx: i64) {
    if re_ptr.is_null() {
        return;
    }
    unsafe { as_regex_mut(re_ptr) }.last_index = idx;
}
