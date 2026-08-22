//! Spec `StringToBigInt` (ES §7.1.14) — strict-grammar parse.
//!
//! Unlike [`crate::construct::__torajs_bigint_from_str`] (lenient:
//! silently skips non-digit bytes, lexer-filtered input expected),
//! this entry enforces the `StringIntegerLiteral` grammar and
//! answers NULL for any string outside it — the loose-equality
//! ladder (§7.2.14 step 7/8, `BigInt == String`) maps NULL to
//! "undefined → compare is false". Grammar:
//!
//! ```text
//! StringIntegerLiteral ::: StrWhiteSpace_opt
//!                          StringIntegerLiteralPart_opt
//!                          StrWhiteSpace_opt
//! Part ::: SignedDecimalDigits | 0x HexDigits | 0o OctalDigits
//!          | 0b BinaryDigits          (no sign on radix forms)
//! ```
//!
//! Empty / whitespace-only input is `0n` per spec. No decimal
//! point, no exponent, no `Infinity`, no `n` suffix.

use core::ffi::c_void;

use crate::internal::{
    add_u32_inplace, alloc_raw, mul_u32_inplace, normalize, read_len, write_sign,
};
use crate::layout::{STR_HDR_SIZE, STR_LEN_OFF};

/// StrWhiteSpaceChar per §12.2 / §12.3: ASCII ws + the common
/// UTF-8 encoded WhiteSpace / LineTerminator code points test262
/// exercises (NBSP U+00A0, BOM U+FEFF, LS U+2028, PS U+2029,
/// OGHAM U+1680, the U+2000-U+200A range, U+202F, U+205F, U+3000).
/// Returns the byte length of the whitespace sequence at `i`
/// (0 = not whitespace).
fn ws_len(b: &[u8], i: usize) -> usize {
    match b[i] {
        b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C => 1,
        0xC2 if i + 1 < b.len() && b[i + 1] == 0xA0 => 2, // U+00A0
        0xE1 if i + 2 < b.len() && b[i + 1] == 0x9A && b[i + 2] == 0x80 => 3, // U+1680
        0xE2 if i + 2 < b.len() => match (b[i + 1], b[i + 2]) {
            (0x80, 0x80..=0x8A) => 3,         // U+2000-U+200A
            (0x80, 0xA8) | (0x80, 0xA9) => 3, // U+2028 / U+2029
            (0x80, 0xAF) => 3,                // U+202F
            (0x81, 0x9F) => 3,                // U+205F
            _ => 0,
        },
        0xE3 if i + 2 < b.len() && b[i + 1] == 0x80 && b[i + 2] == 0x80 => 3, // U+3000
        0xEF if i + 2 < b.len() && b[i + 1] == 0xBB && b[i + 2] == 0xBF => 3, // U+FEFF
        _ => 0,
    }
}

fn hex_val(c: u8) -> Option<u32> {
    match c {
        b'0'..=b'9' => Some((c - b'0') as u32),
        b'a'..=b'f' => Some((c - b'a' + 10) as u32),
        b'A'..=b'F' => Some((c - b'A' + 10) as u32),
        _ => None,
    }
}

/// `StringToBigInt(s)` per ES §7.1.14. `s` is a Str heap pointer
/// (or NULL = empty). Returns a freshly-owned BigInt, or NULL when
/// the string is not a valid `StringIntegerLiteral` (spec answer
/// "undefined").
///
/// # Safety
///
/// `s` is either NULL or a valid Str heap block pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_from_str_strict(s: *const c_void) -> *mut u8 {
    if s.is_null() {
        let z = unsafe { alloc_raw(0) };
        unsafe { normalize(z) };
        return z;
    }
    let len = unsafe { *((s as *const u8).add(STR_LEN_OFF) as *const u32) } as usize;
    let body = unsafe { core::slice::from_raw_parts((s as *const u8).add(STR_HDR_SIZE), len) };

    // Trim leading / trailing StrWhiteSpace.
    let mut lo = 0usize;
    while lo < body.len() {
        let n = ws_len(body, lo);
        if n == 0 {
            break;
        }
        lo += n;
    }
    let mut hi = body.len();
    // Trailing trim: rescan forward from lo, remembering the last
    // non-ws end (multi-byte ws makes backward scanning unreliable).
    {
        let mut i = lo;
        let mut last_end = lo;
        while i < hi {
            let n = ws_len(body, i);
            if n == 0 {
                i += 1;
                last_end = i;
            } else {
                i += n;
            }
        }
        // Whitespace is only allowed at the edges — an interior ws
        // run followed by more content is caught by the digit
        // filters below (ws bytes aren't digits), so `hi` may only
        // shrink past pure trailing ws.
        hi = last_end;
    }
    let t = &body[lo..hi];

    // Empty (or whitespace-only) → 0n.
    if t.is_empty() {
        let z = unsafe { alloc_raw(0) };
        unsafe { normalize(z) };
        return z;
    }

    // Optional sign — decimal form only.
    let (negative, rest) = match t[0] {
        b'-' => (true, &t[1..]),
        b'+' => (false, &t[1..]),
        _ => (false, t),
    };
    let signed = negative || t[0] == b'+';

    // Radix-prefixed forms (sign not allowed by the grammar).
    if rest.len() >= 2 && rest[0] == b'0' {
        let (radix, digits): (u32, &[u8]) = match rest[1] {
            b'x' | b'X' => (16, &rest[2..]),
            b'o' | b'O' => (8, &rest[2..]),
            b'b' | b'B' => (2, &rest[2..]),
            _ => (0, rest),
        };
        if radix != 0 {
            // Validate before allocating — an early reject must not
            // leak the accumulator.
            if signed
                || digits.is_empty()
                || !digits
                    .iter()
                    .all(|&c| matches!(hex_val(c), Some(d) if d < radix))
            {
                return core::ptr::null_mut();
            }
            let mut r = unsafe { alloc_raw(0) };
            for &c in digits {
                let d = hex_val(c).unwrap_or(0);
                unsafe {
                    mul_u32_inplace(&mut r, radix);
                    add_u32_inplace(&mut r, d);
                }
            }
            unsafe { normalize(r) };
            return r;
        }
    }

    // Decimal form: at least one digit, digits only. Validate
    // before allocating (leak-free early reject).
    if rest.is_empty() || !rest.iter().all(|c| c.is_ascii_digit()) {
        return core::ptr::null_mut();
    }
    let mut r = unsafe { alloc_raw(0) };
    for &c in rest {
        unsafe {
            mul_u32_inplace(&mut r, 10);
            add_u32_inplace(&mut r, (c - b'0') as u32);
        }
    }
    unsafe { normalize(r) };
    if negative && unsafe { read_len(r) } > 0 {
        unsafe { write_sign(r, 1) };
    }
    r
}
