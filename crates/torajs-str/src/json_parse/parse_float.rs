//! `JSON.parse` floating-point literal parser.

use super::{json_skip_ws, json_throw, str_payload};

unsafe extern "C" {
    fn strtod(s: *const i8, endp: *mut *mut i8) -> f64;
}

/// Parse a JSON number literal — supports `-` sign, fraction `.`,
/// and exponent `e[+-]?[0-9]+`. Scans the span first, copies into a
/// stack buffer for libc `strtod`. Matches the pre-port C runtime's
/// exact strtod conversion (Rust's `f64::from_str` is bit-identical
/// in practice but we keep the libc path for byte-equal porting).
///
/// # Safety
/// `str_ptr` valid Str heap block; `pos` writable i64.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_parse_float(str_ptr: *const u8, pos: *mut i64) -> f64 {
    let data = unsafe { str_payload(str_ptr) };
    let p = unsafe { &mut *pos };
    json_skip_ws(data, p);
    let start = *p as usize;
    let mut end = start;
    if end < data.len() && data[end] == b'-' {
        end += 1;
    }
    while end < data.len() && data[end].is_ascii_digit() {
        end += 1;
    }
    if end < data.len() && data[end] == b'.' {
        end += 1;
        while end < data.len() && data[end].is_ascii_digit() {
            end += 1;
        }
    }
    if end < data.len() && (data[end] == b'e' || data[end] == b'E') {
        end += 1;
        if end < data.len() && (data[end] == b'+' || data[end] == b'-') {
            end += 1;
        }
        while end < data.len() && data[end].is_ascii_digit() {
            end += 1;
        }
    }
    let bare_minus = end == start + 1 && data[start] == b'-';
    if end == start || bare_minus {
        json_throw("JSON.parse: expected number digits", start as i64);
        return 0.0;
    }
    // Copy span into a 64-byte stack NUL-buffer for strtod.
    let mut buf = [0i8; 64];
    let mut span_len = end - start;
    if span_len >= buf.len() {
        span_len = buf.len() - 1;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(
            data.as_ptr().add(start),
            buf.as_mut_ptr() as *mut u8,
            span_len,
        );
        buf[span_len] = 0;
    }
    *p = end as i64;
    unsafe { strtod(buf.as_ptr(), core::ptr::null_mut()) }
}
