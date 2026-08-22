//! `JSON.parse` string-literal parser — two-pass (scan + alloc +
//! write) so the result Str fits in one allocation.

use super::{JsonSrc, json_skip_ws, json_src, json_throw};
use crate::block::StrBlock;

unsafe extern "C" {
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
}

/// Hex value of an ASCII hex digit, or `None`.
#[inline]
fn hex_digit(c: u8) -> Option<u32> {
    match c {
        b'0'..=b'9' => Some((c - b'0') as u32),
        b'a'..=b'f' => Some((c - b'a' + 10) as u32),
        b'A'..=b'F' => Some((c - b'A' + 10) as u32),
        _ => None,
    }
}

/// The code unit a `\uXXXX` escape at `i` (the backslash) denotes.
/// Non-hex digits contribute nothing, matching the pre-port reader.
#[inline]
fn u_escape_unit(data: &JsonSrc, i: usize) -> u16 {
    let mut v: u32 = 0;
    for k in 0..4 {
        v <<= 4;
        if let Some(d) = hex_digit(data.ascii(i + 2 + k)) {
            v |= d;
        }
    }
    v as u16
}

/// Parse a JSON string literal — opens `"`, decodes escape sequences
/// (`\"`, `\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t`, `\uXXXX`), closes
/// `"`. Throws on bad escape / unterminated string.
///
/// Two-pass: scan to find the closing quote, count decoded code
/// units and find the widest one, then allocate the matching
/// encoding and write. Single allocation.
///
/// The result carries code units, not bytes. A `\uXXXX` escape used
/// to be truncated to its low 8 bits — a pre-P11.1 byte-Str view
/// that turned `"中"` into one Latin-1 byte — and a source
/// already holding units above 0xFF was read as raw bytes, which is
/// half of its payload. Both are one question now: what unit is at
/// this index, and how wide does the widest one make the answer.
///
/// # Safety
/// `str_ptr` is a valid Str heap block; `pos` is a writable i64.
/// Returned pointer is a fresh refcount=1 Str.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_parse_string(str_ptr: *const u8, pos: *mut i64) -> *mut u8 {
    let data = unsafe { json_src(str_ptr) };
    let p = unsafe { &mut *pos };
    json_skip_ws(&data, p);
    let start = *p;
    if (*p as usize) >= data.len() || data.ascii(*p as usize) != b'"' {
        json_throw("JSON.parse: expected string", start);
        return unsafe { __torajs_str_alloc_pooled(0) };
    }
    *p += 1;

    // Pass 1: find the closing quote, count decoded units, and note
    // whether any of them needs more than a byte.
    let mut out_len: u32 = 0;
    let mut widest: u16 = 0;
    let mut scan = *p as usize;
    while scan < data.len() {
        let c = data.unit(scan);
        if c == b'"' as u16 {
            break;
        }
        if c == b'\\' as u16 {
            if scan + 1 >= data.len() {
                json_throw("JSON.parse: bad escape", scan as i64);
                return unsafe { __torajs_str_alloc_pooled(0) };
            }
            if data.ascii(scan + 1) == b'u' {
                if scan + 6 > data.len() {
                    json_throw("JSON.parse: short \\u escape", scan as i64);
                    return unsafe { __torajs_str_alloc_pooled(0) };
                }
                widest = widest.max(u_escape_unit(&data, scan));
                out_len += 1;
                scan += 6;
            } else {
                widest = widest.max(data.unit(scan + 1));
                out_len += 1;
                scan += 2;
            }
            continue;
        }
        widest = widest.max(c);
        out_len += 1;
        scan += 1;
    }
    if scan >= data.len() {
        json_throw("JSON.parse: unterminated string", start);
        return unsafe { __torajs_str_alloc_pooled(0) };
    }

    // Pass 2: alloc the encoding the widest unit asks for, write.
    let latin1 = widest <= 0xFF;
    let mut block = StrBlock::alloc_with_encoding(out_len, latin1);
    let payload_bytes = if latin1 { out_len } else { out_len * 2 };
    let out = unsafe { block.as_bytes_mut(payload_bytes) };
    let mut j = 0usize;
    let mut i = *p as usize;
    let mut put = |unit: u16, j: &mut usize| {
        if latin1 {
            out[*j] = unit as u8;
        } else {
            out[*j * 2..*j * 2 + 2].copy_from_slice(&unit.to_le_bytes());
        }
        *j += 1;
    };
    while i < scan {
        let c = data.unit(i);
        if c != b'\\' as u16 {
            put(c, &mut j);
            i += 1;
            continue;
        }
        let e = data.ascii(i + 1);
        match e {
            b'"' => {
                put(b'"' as u16, &mut j);
                i += 2;
            }
            b'\\' => {
                put(b'\\' as u16, &mut j);
                i += 2;
            }
            b'/' => {
                put(b'/' as u16, &mut j);
                i += 2;
            }
            b'b' => {
                put(0x08, &mut j);
                i += 2;
            }
            b'f' => {
                put(0x0c, &mut j);
                i += 2;
            }
            b'n' => {
                put(b'\n' as u16, &mut j);
                i += 2;
            }
            b'r' => {
                put(b'\r' as u16, &mut j);
                i += 2;
            }
            b't' => {
                put(b'\t' as u16, &mut j);
                i += 2;
            }
            b'u' => {
                put(u_escape_unit(&data, i), &mut j);
                i += 6;
            }
            _ => {
                put(data.unit(i + 1), &mut j);
                i += 2;
            }
        }
    }
    *p = (scan + 1) as i64; // skip closing quote
    block.into_raw()
}
