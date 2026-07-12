//! String → Number coercion (ES spec §7.1.4 / §7.1.4.1.1).
//!
//! Single FFI entry point [`__torajs_str_to_number`] mirroring the
//! pre-rewrite C `__torajs_str_to_number(const void *p) -> double`.
//! Called by anyvalue's `ToNumber` path when the operand is
//! `Type::Str` (e.g. `"3.14" + 1` triggers ToNumber on the string
//! first). Returns NaN on any parse failure — matches JS spec
//! exactly.
//!
//! Splits into two layers:
//!
//! - [`parse_number`] — pure Rust slice → `f64`. Trims whitespace,
//!   recognizes `Infinity` / `-Infinity` / `+Infinity` / `NaN`,
//!   otherwise delegates to `f64::from_str` via a stack buffer.
//!   No unsafe, no allocator unless the input doesn't fit in the
//!   stack buffer.
//! - [`__torajs_str_to_number`] — thin extern "C" wrapper that
//!   reads the Str layout (`len` at offset 8 + payload at offset
//!   16) and delegates.
//!
//! Per pillar 2 (自研) + pillar 4 (规范): rewriting `strtod` from
//! scratch would balloon scope. The fallback to `f64::from_str`
//! delegates to `core` (Rust's textbook fastpath / Grisu / Eisel-
//! Lemire choice; identical accuracy guarantees to libc strtod).
//! No external dependency. The trimmed input is at most 64 bytes
//! for the common case (Number literals are short); a `tmp[u8; 64]`
//! stack buffer + UTF-8 conversion is allocation-free for ≤63
//! payload bytes (the same fast path C used).

use core::ffi::c_void;

use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF};
use crate::transform::trim::{is_trim_ws, is_trim_ws_u16};
use torajs_rc::HeapHeader;

/// String → Number per ES spec §7.1.4. Pure Rust core; the FFI
/// wrapper [`__torajs_str_to_number`] reads the Str layout and
/// hands the byte slice here.
///
/// Returns f64::NAN on any parse failure; identical semantics to
/// the pre-rewrite C `strtod`-with-`endp`-must-be-end check.
pub fn parse_number(bytes: &[u8]) -> f64 {
    // Trim leading/trailing whitespace (Latin-1 projection of the
    // ES TrimString set — shared predicate with `s.trim()`).
    let mut s = 0usize;
    let mut e = bytes.len();
    while s < e && is_trim_ws(bytes[s]) {
        s += 1;
    }
    while e > s && is_trim_ws(bytes[e - 1]) {
        e -= 1;
    }
    if s == e {
        // Empty (after trim) → 0, matching spec § ToNumber("").
        return 0.0;
    }
    let slice = &bytes[s..e];

    // Special literals before f64::from_str. (from_str doesn't
    // recognize "Infinity"/"+Infinity" — only "inf" / "infinity"
    // case-insensitive — and matches "NaN" / "nan" only case-
    // insensitive, so we route them through explicit checks to
    // match the ES spec's exact-case wording.)
    if slice == b"Infinity" || slice == b"+Infinity" {
        return f64::INFINITY;
    }
    if slice == b"-Infinity" {
        return f64::NEG_INFINITY;
    }
    if slice == b"NaN" {
        return f64::NAN;
    }

    // Hex / binary / octal prefix per ES spec §7.1.4.1.1
    // (HexIntegerLiteral / BinaryIntegerLiteral / OctalIntegerLiteral):
    // `0x` / `0X` + ≥1 hex digit, `0b` / `0B` + ≥1 binary digit,
    // `0o` / `0O` + ≥1 octal digit. C `strtod` accepted hex natively;
    // `f64::from_str` doesn't recognize any of the three. JS Number is
    // f64 so values past `u64::MAX` lose precision the same way
    // `strtod` would. Empty digit string → NaN (matches strtod's "no
    // conversion performed" → end pointer unchanged → NaN here).
    if slice.len() > 2 && slice[0] == b'0' {
        let radix = match slice[1] {
            b'x' | b'X' => Some(16),
            b'b' | b'B' => Some(2),
            b'o' | b'O' => Some(8),
            _ => None,
        };
        if let Some(r) = radix {
            let digits = &slice[2..];
            if digits.is_empty() {
                return f64::NAN;
            }
            let digits_str = match core::str::from_utf8(digits) {
                Ok(s) => s,
                Err(_) => return f64::NAN,
            };
            return match u64::from_str_radix(digits_str, r) {
                Ok(n) => n as f64,
                Err(_) => f64::NAN,
            };
        }
    }

    // Decode bytes as ASCII (per the str-pool ABI, all source
    // bytes are u8 already; UTF-8 multi-byte sequences for digits
    // would also parse correctly via str::from_utf8 + f64::from_
    // str, but they don't appear in legitimate number literals).
    // Fail fast on non-UTF-8 → NaN.
    let s = match core::str::from_utf8(slice) {
        Ok(s) => s,
        Err(_) => return f64::NAN,
    };

    // f64::from_str's contract: "must consume the entire input"
    // — matches the C original's `endp != end → NaN` check.
    s.parse::<f64>().unwrap_or(f64::NAN)
}

/// UTF-16 LE variant of [`parse_number`]: trims the full ES
/// TrimString set per code unit, then narrows the remainder to
/// ASCII bytes for the shared byte-slice core. Any unit > 0x7F in
/// the trimmed body → NaN (a StringNumericLiteral is ASCII-only).
pub fn parse_number_utf16(payload: &[u8]) -> f64 {
    let unit_at = |i: usize| u16::from_le_bytes([payload[i], payload[i + 1]]);
    let mut s = 0usize;
    let mut e = payload.len() & !1;
    while s < e && is_trim_ws_u16(unit_at(s)) {
        s += 2;
    }
    while e > s && is_trim_ws_u16(unit_at(e - 2)) {
        e -= 2;
    }
    let mut ascii = alloc::vec::Vec::with_capacity((e - s) / 2);
    let mut i = s;
    while i < e {
        let u = unit_at(i);
        if u > 0x7f {
            return f64::NAN;
        }
        ascii.push(u as u8);
        i += 2;
    }
    parse_number(&ascii)
}

/// Mirrors the pre-rewrite C `__torajs_str_to_number(const void *p)
/// -> double`. Null input returns 0.0 (matches the C guard).
/// Encoding-aware: a Latin-1 payload parses in place; a UTF-16 LE
/// payload routes through [`parse_number_utf16`].
///
/// # Safety
///
/// `p` must be null or point at a valid Str block whose layout
/// matches [`crate::layout`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_to_number(p: *const c_void) -> f64 {
    if p.is_null() {
        return 0.0;
    }
    // SAFETY: caller's invariant.
    let len = unsafe { (p.cast::<u8>().add(STR_LEN_OFF) as *const u32).read() } as usize;
    let header = unsafe { &*(p as *const HeapHeader) };
    if (header.flags & STR_FLAG_IS_LATIN1) != 0 {
        // SAFETY: Latin-1 payload is `len` bytes.
        let bytes = unsafe { core::slice::from_raw_parts(p.cast::<u8>().add(STR_DATA_OFF), len) };
        parse_number(bytes)
    } else {
        // SAFETY: UTF-16 payload is `len` code units = `len * 2` bytes.
        let payload =
            unsafe { core::slice::from_raw_parts(p.cast::<u8>().add(STR_DATA_OFF), len * 2) };
        parse_number_utf16(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::StrBlock;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn make_str(bytes: &[u8]) -> StrBlock {
        let mut block = StrBlock::alloc(bytes.len() as u32);
        unsafe {
            block
                .as_bytes_mut(bytes.len() as u32)
                .copy_from_slice(bytes)
        };
        block
    }

    fn assert_to_number(s: &[u8], expected: f64) {
        let _g = TEST_LOCK.lock().unwrap();
        crate::pool::clear_for_test();
        let block = make_str(s);
        let got = unsafe { __torajs_str_to_number(block.0.as_ptr() as *const c_void) };
        if expected.is_nan() {
            assert!(
                got.is_nan(),
                "expected NaN for {:?} got {got}",
                core::str::from_utf8(s).unwrap_or("<non-utf8>")
            );
        } else {
            assert_eq!(
                got,
                expected,
                "{:?} → {got} (want {expected})",
                core::str::from_utf8(s).unwrap_or("<non-utf8>")
            );
        }
        block.free_pool_aware();
    }

    #[test]
    fn null_returns_zero() {
        assert_eq!(unsafe { __torajs_str_to_number(core::ptr::null()) }, 0.0);
    }

    #[test]
    fn empty_string_returns_zero() {
        assert_to_number(b"", 0.0);
    }

    #[test]
    fn whitespace_only_returns_zero() {
        assert_to_number(b"   ", 0.0);
        assert_to_number(b"\t\n\r", 0.0);
    }

    #[test]
    fn plain_integer() {
        assert_to_number(b"42", 42.0);
        assert_to_number(b"-1", -1.0);
        assert_to_number(b"0", 0.0);
    }

    #[test]
    fn float_literal() {
        assert_to_number(b"3.14", 3.14);
        assert_to_number(b"-0.5", -0.5);
        assert_to_number(b"1e10", 1e10);
        assert_to_number(b"-2.5e-3", -2.5e-3);
    }

    #[test]
    fn surrounding_whitespace_trimmed() {
        assert_to_number(b"  42  ", 42.0);
        assert_to_number(b"\t3.14\n", 3.14);
        // NBSP (0xA0) is in the Latin-1 TrimString projection.
        assert_to_number(b"\xa042\xa0", 42.0);
        assert_to_number(b"\xa0\xa0", 0.0);
    }

    #[test]
    fn utf16_payload_trims_unicode_ws() {
        // "\u{3000}42\u{FEFF}" as UTF-16 LE units.
        let payload = [0x00u8, 0x30, 0x34, 0x00, 0x32, 0x00, 0xff, 0xfe];
        assert_eq!(parse_number_utf16(&payload), 42.0);
        // All-whitespace → 0.
        let ws = [0x00u8, 0x30, 0x28, 0x20];
        assert_eq!(parse_number_utf16(&ws), 0.0);
        // Non-ASCII in the trimmed body → NaN.
        let bad = [0x34u8, 0x00, 0x00, 0x4e];
        assert!(parse_number_utf16(&bad).is_nan());
    }

    #[test]
    fn infinity_literals() {
        assert_to_number(b"Infinity", f64::INFINITY);
        assert_to_number(b"+Infinity", f64::INFINITY);
        assert_to_number(b"-Infinity", f64::NEG_INFINITY);
        assert_to_number(b"  Infinity  ", f64::INFINITY);
    }

    #[test]
    fn nan_literal() {
        assert_to_number(b"NaN", f64::NAN);
    }

    #[test]
    fn invalid_input_returns_nan() {
        assert_to_number(b"abc", f64::NAN);
        assert_to_number(b"42abc", f64::NAN);
        assert_to_number(b"3.14.15", f64::NAN);
    }

    #[test]
    fn pure_core_parse_number_no_alloc() {
        // Exercise the Rust core directly to confirm it doesn't
        // need the FFI wrapper's layout reads.
        assert_eq!(parse_number(b"100"), 100.0);
        assert!(parse_number(b"NaN").is_nan());
        assert_eq!(parse_number(b"  -2.5  "), -2.5);
    }

    #[test]
    fn hex_prefix() {
        // strtod-compatible: 0x / 0X prefix → hex int → f64.
        // Regression test for unary-plus-minus-on-string-001.
        assert_to_number(b"0xff", 255.0);
        assert_to_number(b"0xFF", 255.0);
        assert_to_number(b"0Xff", 255.0);
        assert_to_number(b"0x0", 0.0);
        assert_to_number(b"0x10", 16.0);
        assert_to_number(b"  0xff  ", 255.0);
    }

    #[test]
    fn hex_prefix_empty_digits_is_nan() {
        // "0x" alone (no digits after prefix) → NaN, matches strtod.
        assert_to_number(b"0x", f64::NAN);
        assert_to_number(b"0X", f64::NAN);
    }

    #[test]
    fn hex_prefix_invalid_digit() {
        assert_to_number(b"0xZZ", f64::NAN);
        assert_to_number(b"0x12g", f64::NAN);
    }

    #[test]
    fn binary_prefix() {
        // ES §7.1.4.1.1 BinaryIntegerLiteral.
        assert_to_number(b"0b101", 5.0);
        assert_to_number(b"0B101", 5.0);
        assert_to_number(b"0b0", 0.0);
        assert_to_number(b"0b11111111", 255.0);
        assert_to_number(b"  0b101  ", 5.0);
    }

    #[test]
    fn binary_prefix_empty_or_invalid() {
        assert_to_number(b"0b", f64::NAN);
        assert_to_number(b"0b2", f64::NAN);
        assert_to_number(b"0bff", f64::NAN);
    }

    #[test]
    fn octal_prefix() {
        // ES §7.1.4.1.1 OctalIntegerLiteral.
        assert_to_number(b"0o17", 15.0);
        assert_to_number(b"0O17", 15.0);
        assert_to_number(b"0o0", 0.0);
        assert_to_number(b"0o777", 511.0);
        assert_to_number(b"  0o17  ", 15.0);
    }

    #[test]
    fn octal_prefix_empty_or_invalid() {
        assert_to_number(b"0o", f64::NAN);
        assert_to_number(b"0o8", f64::NAN);
        assert_to_number(b"0o9", f64::NAN);
        assert_to_number(b"0oZ", f64::NAN);
    }
}
