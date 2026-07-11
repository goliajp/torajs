//! `Array.from(s)` for string sources — port of
//! `runtime_str.c` L1613-1623, encoding-aware since RFC 20260711
//! follow-up.
//!
//! ES §23.1.2.1 iterates the string's CODE POINTS (the string
//! iterator groups surrogate pairs): `Array.from("𝄞a")` is
//! `["𝄞", "a"]`, two elements. The pre-fix shape emitted one
//! single-byte Str per PAYLOAD BYTE — correct only for Latin-1
//! sources (byte == unit == cp there, no pairs); UTF-16 sources
//! got their unit low-bytes as elements.
//!
//! Each result Str has rc=1; the array has cap pre-sized to the
//! unit count (≥ element count; surrogate pairs make it shrink).

use crate::alloc::__torajs_arr_alloc;
use crate::grow::__torajs_arr_push;
use crate::join_enc::{STR_DATA_OFF, str_data, str_is_latin1, str_units};
use crate::str_bridge::str_alloc_pooled_enc;

/// `Array.from(s)` over a Str source: one element per code point.
///
/// # Safety
///
/// `s` must be a valid `*const Str` (live, rc > 0). Returned pointer
/// is a fresh refcount=1 `Array<Str>` block whose elements each have
/// rc=1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_from_string(s: *const u8) -> *mut u8 {
    unsafe {
        let units = str_units(s);
        let data = str_data(s);
        let mut arr = __torajs_arr_alloc(units);
        if str_is_latin1(s) {
            // Latin-1: every unit is one byte and one cp.
            for i in 0..units {
                let byte = data.add(i as usize).read();
                let p = str_alloc_pooled_enc(1, true);
                p.add(STR_DATA_OFF).write(byte);
                arr = __torajs_arr_push(arr, p as i64);
            }
            return arr;
        }
        // UTF-16 LE: group surrogate pairs into 2-unit elements
        // (string-iterator cp semantics); narrow lone units ≤ 0xFF
        // back to a Latin-1 element so downstream fast paths keep
        // their 1-byte shape.
        let mut i: u64 = 0;
        while i < units {
            let u = (data.add((i as usize) * 2) as *const u16).read_unaligned();
            let is_pair = (0xD800..=0xDBFF).contains(&u) && i + 1 < units && {
                let lo = (data.add((i as usize + 1) * 2) as *const u16).read_unaligned();
                (0xDC00..=0xDFFF).contains(&lo)
            };
            let p = if is_pair {
                let p = str_alloc_pooled_enc(2, false);
                core::ptr::copy_nonoverlapping(data.add((i as usize) * 2), p.add(STR_DATA_OFF), 4);
                i += 2;
                p
            } else if u <= 0xFF {
                let p = str_alloc_pooled_enc(1, true);
                p.add(STR_DATA_OFF).write(u as u8);
                i += 1;
                p
            } else {
                let p = str_alloc_pooled_enc(1, false);
                core::ptr::copy_nonoverlapping(data.add((i as usize) * 2), p.add(STR_DATA_OFF), 2);
                i += 1;
                p
            };
            arr = __torajs_arr_push(arr, p as i64);
        }
        arr
    }
}
