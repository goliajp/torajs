//! `JSON.parse` floating-point literal parser.

use super::{json_skip_ws, json_throw, str_payload};

unsafe extern "C" {
    // v0.7-A4 Step 15-e: 0-libc string → f64 parser. Replaces
    // libc strtod for the JSON numeric token path.
    fn __torajs_fmt_atod(s: *const u8, len: usize, endp: *mut usize) -> f64;
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
    // v0.7-A4 Step 15-e: parse via torajs-fmt's __torajs_fmt_atod
    // (0-libc; Rust core::str::FromStr's Eisel-Lemire core).
    // Direct pointer + length — no NUL-buffer copy needed.
    let span_len = end - start;
    *p = end as i64;
    let mut endp_ignored: usize = 0;
    unsafe { __torajs_fmt_atod(data.as_ptr().add(start), span_len, &mut endp_ignored) }
}
