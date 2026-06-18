//! RegExp lifecycle externs — drop / get_source / lastIndex
//! get-set. Port of `runtime_regex.c` L1519-1552, L2130-2140.

use alloc::boxed::Box;
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

/// `re.flags` — returns the spec-ordered flag string ("g" / "im" /
/// "gimsuy" / etc.) per ES §22.2.6.4. Order is fixed: g, i, m, s, u, y
/// (we don't support the `d` hasIndices flag). NULL receiver returns
/// an empty string.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`. Returned pointer is a
/// pool-Str with rc=1; caller takes ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_get_flags(re_ptr: *const c_void) -> *mut c_void {
    if re_ptr.is_null() {
        return unsafe { __torajs_str_alloc_pooled(0) as *mut c_void };
    }
    let f = unsafe { as_regex(re_ptr) }.flags;
    // Build the canonical 6-byte buffer at most, ordered g, i, m, s, u, y.
    // Mirrors `flags::flag_bit_for_char` / `parse_flags` source of truth.
    let mut buf = [0u8; 6];
    let mut n = 0usize;
    let bits: [(u8, u8); 6] = [
        (crate::parser::RE_FLAG_G, b'g'),
        (crate::parser::RE_FLAG_I, b'i'),
        (crate::parser::RE_FLAG_M, b'm'),
        (crate::parser::RE_FLAG_S, b's'),
        (crate::parser::RE_FLAG_U, b'u'),
        (crate::parser::RE_FLAG_Y, b'y'),
    ];
    for (bit, ch) in bits {
        if f & bit != 0 {
            buf[n] = ch;
            n += 1;
        }
    }
    let s = unsafe { __torajs_str_alloc_pooled(n as u64) };
    if n > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(buf.as_ptr(), s.add(STR_HDR_SIZE), n);
        }
    }
    s as *mut c_void
}

/// `re.toString()` — per ES §22.2.6.13 returns `/` + source + `/` +
/// flags. Spec-ordered flags (g, i, m, s, u, y) match `__torajs_regex_
/// get_flags`. NULL receiver returns `/(?:)/` (matches V8/JSC fallback).
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`. Returned pointer is a
/// pool-Str with rc=1; caller takes ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_to_string(re_ptr: *const c_void) -> *mut c_void {
    if re_ptr.is_null() {
        // Spec doesn't define this; mirror engines' fallback shape.
        let fallback: &[u8] = b"/(?:)/";
        let s = unsafe { __torajs_str_alloc_pooled(fallback.len() as u64) };
        unsafe {
            core::ptr::copy_nonoverlapping(fallback.as_ptr(), s.add(STR_HDR_SIZE), fallback.len());
        }
        return s as *mut c_void;
    }
    let re = unsafe { as_regex(re_ptr) };
    // Build the canonical flag bytes (matches __torajs_regex_get_flags).
    let mut flag_buf = [0u8; 6];
    let mut nflag = 0usize;
    let bits: [(u8, u8); 6] = [
        (crate::parser::RE_FLAG_G, b'g'),
        (crate::parser::RE_FLAG_I, b'i'),
        (crate::parser::RE_FLAG_M, b'm'),
        (crate::parser::RE_FLAG_S, b's'),
        (crate::parser::RE_FLAG_U, b'u'),
        (crate::parser::RE_FLAG_Y, b'y'),
    ];
    for (bit, ch) in bits {
        if re.flags & bit != 0 {
            flag_buf[nflag] = ch;
            nflag += 1;
        }
    }
    let src_len = re.src_bytes.len();
    let total = 1 + src_len + 1 + nflag;
    let s = unsafe { __torajs_str_alloc_pooled(total as u64) };
    let dst = unsafe { s.add(STR_HDR_SIZE) };
    unsafe {
        *dst = b'/';
        if src_len > 0 {
            core::ptr::copy_nonoverlapping(re.src_bytes.as_ptr(), dst.add(1), src_len);
        }
        *dst.add(1 + src_len) = b'/';
        if nflag > 0 {
            core::ptr::copy_nonoverlapping(flag_buf.as_ptr(), dst.add(2 + src_len), nflag);
        }
    }
    s as *mut c_void
}

/// `re.global` / `.ignoreCase` / `.multiline` / `.dotAll` / `.unicode`
/// / `.sticky` — boolean flag getters per ES §22.2.6.5-10. Returns 1
/// when the corresponding bit is set in `re.flags`, 0 otherwise. NULL
/// receiver returns 0 for every flag (no live RegExp = no flags).
/// `flag_bit` is the `RE_FLAG_*` byte constant the caller emits at
/// compile time so the ssa_lower side doesn't have to re-derive the
/// bit-to-name map.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_has_flag(re_ptr: *const c_void, flag_bit: i64) -> bool {
    if re_ptr.is_null() {
        return false;
    }
    (unsafe { as_regex(re_ptr) }.flags & (flag_bit as u8)) != 0
}
