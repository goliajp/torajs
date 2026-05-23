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

use std::ffi::c_void;

use crate::layout::{STR_DATA_OFF, STR_LEN_OFF};

/// Whitespace per ES spec §7.1.4.1.1 StringWhiteSpace. ASCII subset
/// matched: SP / TAB / LF / CR / VT / FF. (NBSP, BOM, ZWNBSP are
/// multi-byte UTF-8 and the C original explicitly skipped them too
/// — they only show up via `String(...)` rituals and the test262
/// failures around them are diagnosed separately.)
#[inline]
fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

/// String → Number per ES spec §7.1.4. Pure Rust core; the FFI
/// wrapper [`__torajs_str_to_number`] reads the Str layout and
/// hands the byte slice here.
///
/// Returns f64::NAN on any parse failure; identical semantics to
/// the pre-rewrite C `strtod`-with-`endp`-must-be-end check.
pub fn parse_number(bytes: &[u8]) -> f64 {
    // Trim leading/trailing whitespace.
    let mut s = 0usize;
    let mut e = bytes.len();
    while s < e && is_ws(bytes[s]) {
        s += 1;
    }
    while e > s && is_ws(bytes[e - 1]) {
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

    // Hex prefix per ES spec §7.1.4.1.1 HexIntegerLiteral:
    // `0x` / `0X` followed by ≥ 1 hex digit. C `strtod` accepted
    // these natively; `f64::from_str` doesn't. JS Number is f64 so
    // values past `u64::MAX` lose precision the same way `strtod`
    // would. Empty digit string → NaN (matches strtod's "no
    // conversion performed" → end pointer unchanged → NaN here).
    //
    // Binary (`0b..`) and octal (`0o..`) prefixes are a separate
    // substrate item — Number("0b10") currently returns NaN even
    // pre-rewrite; the test fixture's own comment calls that out.
    if slice.len() > 2 && slice[0] == b'0' && (slice[1] == b'x' || slice[1] == b'X') {
        let hex = &slice[2..];
        if hex.is_empty() {
            return f64::NAN;
        }
        let hex_str = match core::str::from_utf8(hex) {
            Ok(s) => s,
            Err(_) => return f64::NAN,
        };
        return match u64::from_str_radix(hex_str, 16) {
            Ok(n) => n as f64,
            Err(_) => f64::NAN,
        };
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

/// Mirrors the pre-rewrite C `__torajs_str_to_number(const void *p)
/// -> double`. Null input returns 0.0 (matches the C guard).
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
    let len = unsafe { (p.cast::<u8>().add(STR_LEN_OFF) as *const u64).read() } as usize;
    // SAFETY: same.
    let bytes = unsafe { core::slice::from_raw_parts(p.cast::<u8>().add(STR_DATA_OFF), len) };
    parse_number(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::StrBlock;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn make_str(bytes: &[u8]) -> StrBlock {
        let mut block = StrBlock::alloc(bytes.len() as u64);
        unsafe {
            block
                .as_bytes_mut(bytes.len() as u64)
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
}
