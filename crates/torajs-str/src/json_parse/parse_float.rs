//! `JSON.parse` floating-point literal parser.

use super::{json_skip_ws, json_src, json_throw};

unsafe extern "C" {
    // v0.7-A4 Step 15-e: 0-libc string → f64 parser. Replaces
    // libc strtod for the JSON numeric token path.
    fn __torajs_fmt_atod(s: *const u8, len: usize, endp: *mut usize) -> f64;
}

/// Parse a JSON number literal, `-? (0 | [1-9][0-9]*) (. [0-9]+)?
/// ([eE] [+-]? [0-9]+)?` (ECMA-404 §8, ECMA-262 §25.5.1), and
/// convert the span with `__torajs_fmt_atod`.
///
/// A part the grammar opens must have its digits: `4.`, `1e` and
/// `-` throw. A leading `0` is the whole integer part, so `01` ends
/// the token after the `0` and the unit left over fails whatever
/// reads next (the end-of-text check, or `,` / `]` / `}`).
///
/// # Safety
/// `str_ptr` valid Str heap block; `pos` writable i64.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_parse_float(str_ptr: *const u8, pos: *mut i64) -> f64 {
    let data = unsafe { json_src(str_ptr) };
    let p = unsafe { &mut *pos };
    json_skip_ws(&data, p);
    let start = *p as usize;
    let at = |i: usize| if i < data.len() { data.ascii(i) } else { 0 };
    let digits_from = |mut i: usize| {
        while at(i).is_ascii_digit() {
            i += 1;
        }
        i
    };
    let mut end = start;
    if at(end) == b'-' {
        end += 1;
    }
    end = match at(end) {
        b'0' => end + 1,
        b'1'..=b'9' => digits_from(end),
        _ => {
            json_throw("JSON.parse: expected number digits", end as i64);
            return 0.0;
        }
    };
    if at(end) == b'.' {
        let frac = digits_from(end + 1);
        if frac == end + 1 {
            json_throw("JSON.parse: expected fraction digits", frac as i64);
            return 0.0;
        }
        end = frac;
    }
    if at(end) == b'e' || at(end) == b'E' {
        let mut exp = end + 1;
        if at(exp) == b'+' || at(exp) == b'-' {
            exp += 1;
        }
        let digits_end = digits_from(exp);
        if digits_end == exp {
            json_throw("JSON.parse: expected exponent digits", exp as i64);
            return 0.0;
        }
        end = digits_end;
    }
    // v0.7-A4 Step 15-e: parse via torajs-fmt's __torajs_fmt_atod
    // (0-libc; Rust core::str::FromStr's Eisel-Lemire core).
    // Direct pointer + length — no NUL-buffer copy needed.
    let span_len = end - start;
    *p = end as i64;
    let mut endp_ignored: usize = 0;
    if data.is_latin1() {
        // One byte per unit, so the span already reads as the ASCII
        // digits it is — hand `atod` the payload directly.
        return unsafe { __torajs_fmt_atod(data.base().add(start), span_len, &mut endp_ignored) };
    }
    // UTF-16 payload: the digits are two bytes apiece there. Gather
    // the span's low halves, which is what a numeric token can hold.
    let mut span = alloc::vec::Vec::with_capacity(span_len);
    for i in start..end {
        span.push(data.ascii(i));
    }
    unsafe { __torajs_fmt_atod(span.as_ptr(), span_len, &mut endp_ignored) }
}
