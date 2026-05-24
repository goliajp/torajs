//! `JSON.parse` integer-literal parser.

use super::{json_skip_ws, json_throw, str_payload};

/// Parse a JSON integer literal (`-?[0-9]+`). Throws on missing
/// digits.
///
/// # Safety
/// `str_ptr` is a valid Str heap block; `pos` is a writable i64
/// (the per-parse cursor alloca'd by the recursive-descent caller).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_parse_int(str_ptr: *const u8, pos: *mut i64) -> i64 {
    let data = unsafe { str_payload(str_ptr) };
    let p = unsafe { &mut *pos };
    json_skip_ws(data, p);
    let start = *p;
    let mut neg = false;
    if (*p as usize) < data.len() && data[*p as usize] == b'-' {
        neg = true;
        *p += 1;
    }
    let digits_start = *p;
    let mut value: i64 = 0;
    while (*p as usize) < data.len() {
        let c = data[*p as usize];
        if !c.is_ascii_digit() {
            break;
        }
        value = value * 10 + (c - b'0') as i64;
        *p += 1;
    }
    if *p == digits_start {
        json_throw("JSON.parse: expected number digits", start);
        return 0;
    }
    if neg { -value } else { value }
}
