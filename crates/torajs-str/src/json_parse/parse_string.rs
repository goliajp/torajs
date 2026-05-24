//! `JSON.parse` string-literal parser — two-pass (scan + alloc +
//! write) so the result Str fits in one allocation.

use super::{json_skip_ws, json_throw, str_payload};
use crate::alloc::StrBlock;

unsafe extern "C" {
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
}

/// Parse a JSON string literal — opens `"`, decodes escape sequences
/// (`\"`, `\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t`, `\uXXXX`), closes
/// `"`. Throws on bad escape / unterminated string.
///
/// Two-pass: scan to find the closing quote and count decoded
/// length, then alloc + write decoded bytes. Single allocation.
///
/// `\uXXXX` escapes truncate to the low 8 bits — matches the
/// pre-port C runtime's byte-Str view (full UTF-16 surrogate pair
/// decoding is a later wedge once Str ports to UTF-16 / WTF-16).
///
/// # Safety
/// `str_ptr` is a valid Str heap block; `pos` is a writable i64.
/// Returned pointer is a fresh refcount=1 Str.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_parse_string(str_ptr: *const u8, pos: *mut i64) -> *mut u8 {
    let data = unsafe { str_payload(str_ptr) };
    let p = unsafe { &mut *pos };
    json_skip_ws(data, p);
    let start = *p;
    if (*p as usize) >= data.len() || data[*p as usize] != b'"' {
        json_throw("JSON.parse: expected string", start);
        return unsafe { __torajs_str_alloc_pooled(0) };
    }
    *p += 1;

    // Pass 1: find the closing quote and count decoded length.
    let mut out_len: u64 = 0;
    let mut scan = *p as usize;
    while scan < data.len() {
        let c = data[scan];
        if c == b'"' {
            break;
        }
        if c == b'\\' {
            if scan + 1 >= data.len() {
                json_throw("JSON.parse: bad escape", scan as i64);
                return unsafe { __torajs_str_alloc_pooled(0) };
            }
            if data[scan + 1] == b'u' {
                if scan + 6 > data.len() {
                    json_throw("JSON.parse: short \\u escape", scan as i64);
                    return unsafe { __torajs_str_alloc_pooled(0) };
                }
                out_len += 1;
                scan += 6;
            } else {
                out_len += 1;
                scan += 2;
            }
            continue;
        }
        out_len += 1;
        scan += 1;
    }
    if scan >= data.len() {
        json_throw("JSON.parse: unterminated string", start);
        return unsafe { __torajs_str_alloc_pooled(0) };
    }

    // Pass 2: alloc + write decoded bytes.
    let mut block = StrBlock::alloc(out_len);
    let out = unsafe { block.as_bytes_mut(out_len) };
    let mut j = 0usize;
    let mut i = *p as usize;
    while i < scan {
        let c = data[i];
        if c != b'\\' {
            out[j] = c;
            j += 1;
            i += 1;
            continue;
        }
        let e = data[i + 1];
        match e {
            b'"' => {
                out[j] = b'"';
                j += 1;
                i += 2;
            }
            b'\\' => {
                out[j] = b'\\';
                j += 1;
                i += 2;
            }
            b'/' => {
                out[j] = b'/';
                j += 1;
                i += 2;
            }
            b'b' => {
                out[j] = 0x08;
                j += 1;
                i += 2;
            }
            b'f' => {
                out[j] = 0x0c;
                j += 1;
                i += 2;
            }
            b'n' => {
                out[j] = b'\n';
                j += 1;
                i += 2;
            }
            b'r' => {
                out[j] = b'\r';
                j += 1;
                i += 2;
            }
            b't' => {
                out[j] = b'\t';
                j += 1;
                i += 2;
            }
            b'u' => {
                // 4-hex-digit codepoint truncated to low 8 bits —
                // matches pre-port byte-Str view.
                let mut v: u32 = 0;
                for k in 0..4 {
                    let h = data[i + 2 + k];
                    v <<= 4;
                    if h.is_ascii_digit() {
                        v |= (h - b'0') as u32;
                    } else if (b'a'..=b'f').contains(&h) {
                        v |= (h - b'a' + 10) as u32;
                    } else if (b'A'..=b'F').contains(&h) {
                        v |= (h - b'A' + 10) as u32;
                    }
                }
                out[j] = (v & 0xff) as u8;
                j += 1;
                i += 6;
            }
            _ => {
                out[j] = e;
                j += 1;
                i += 2;
            }
        }
    }
    *p = (scan + 1) as i64; // skip closing quote
    block.into_raw()
}
