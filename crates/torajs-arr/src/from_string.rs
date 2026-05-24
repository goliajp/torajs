//! `Array.from(s)` for string sources — port of
//! `runtime_str.c` L1613-1623.
//!
//! Equivalent to `s.split("")` in JS but scoped to tr's byte-Str
//! layout (no UTF-16 / surrogate handling). Returns a fresh
//! `string[]` with one single-byte string per byte of `s`. Each
//! result Str has rc=1; the array has cap pre-sized to `s.len`.

use crate::alloc::__torajs_arr_alloc;
use crate::grow::__torajs_arr_push;
use crate::str_bridge::str_alloc_pooled;

const STR_HDR_SIZE: usize = 16;
const STR_LEN_OFF: usize = 8;

/// `Array.from(s)` over a Str source. Each byte of `s` becomes a
/// fresh single-byte Str element. Cap is pre-sized to `s.len` so
/// the per-element push never triggers a grow.
///
/// # Safety
///
/// `s` must be a valid `*const Str` (live, rc > 0). Returned pointer
/// is a fresh refcount=1 `Array<Str>` block whose elements each have
/// rc=1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_from_string(s: *const u8) -> *mut u8 {
    let s_len = unsafe { (s.add(STR_LEN_OFF) as *const u64).read() };
    let s_data = unsafe { s.add(STR_HDR_SIZE) };
    let mut arr = unsafe { __torajs_arr_alloc(s_len) };
    for i in 0..s_len {
        let byte = unsafe { s_data.add(i as usize).read() };
        let p = unsafe { str_alloc_pooled(1) };
        unsafe { p.add(STR_HDR_SIZE).write(byte) };
        arr = unsafe { __torajs_arr_push(arr, p as i64) };
    }
    arr
}
