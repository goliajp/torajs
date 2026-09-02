//! `JSON.stringify` string-payload escaper — port of `runtime_str.c`
//! L1653-1696.
//!
//! Wraps `s` in `"…"` and replaces JSON-illegal control bytes / the
//! quote / backslash bytes with their escape sequences. Two-pass:
//! pre-compute output length so the result fits in one
//! pool-aware allocation.
//!
//! ## Mapping
//!
//! | byte         | escape   |
//! |--------------|----------|
//! | `"`          | `\"`     |
//! | `\`          | `\\`     |
//! | `\n`         | `\n`     |
//! | `\r`         | `\r`     |
//! | `\t`         | `\t`     |
//! | `\b`         | `\b`     |
//! | `\f`         | `\f`     |
//! | other < 0x20 | `\u00XX` |
//! | else         | pass     |
//!
//! Byte-level: bytes ≥ 0x20 (including UTF-8 continuation / lead
//! bytes) pass through unchanged. A Latin-1 payload has no
//! surrogates in it; the UTF-16 walk is where §25.5.2.2 step 2.d
//! applies, and it escapes the lone ones there.

use crate::block::StrBlock;
use crate::eq::resolve_payload;
use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF};
use torajs_rc::HeapHeader;

const HEX: &[u8; 16] = b"0123456789abcdef";

/// Read the `IS_LATIN1` bit out of a Str heap header (same shape as
/// the eq / concat / print siblings).
///
/// # Safety
///
/// `p` must point at a valid Str block.
#[inline]
unsafe fn str_is_latin1(p: *const u8) -> bool {
    let header = unsafe { &*(p as *const HeapHeader) };
    (header.flags & STR_FLAG_IS_LATIN1) != 0
}

/// Whether the code unit at `i` is half of a well-formed surrogate
/// pair — §25.5.2.2 QuoteJSONString steps 2.c/2.d: a leading
/// surrogate followed by a trailing one, or the trailing half of
/// such a pair. Everything else in the surrogate range is lone and
/// gets a `\uXXXX` escape, because the output is a well-formed
/// string and a lone surrogate has no UTF-8 spelling.
fn surrogate_is_paired(unit: impl Fn(usize) -> u16, n: usize, i: usize) -> bool {
    let u = unit(i);
    if (0xD800..=0xDBFF).contains(&u) {
        return i + 1 < n && (0xDC00..=0xDFFF).contains(&unit(i + 1));
    }
    if (0xDC00..=0xDFFF).contains(&u) {
        return i > 0 && (0xD800..=0xDBFF).contains(&unit(i - 1));
    }
    false
}

/// Quote + escape a UTF-16 payload, staying in UTF-16 (chunk 657 —
/// the byte-walk below mis-read UTF-16 blocks: `length` is code
/// units, the payload is LE pairs, so `"中"` came out as its LE
/// bytes `2d 4e` = `-N`). Escapes are the ASCII set per §25.5.2.2
/// QuoteJSONString, plus the lone surrogates that step 2.d asks
/// for — a paired half passes through, so the walk is per-u16 and
/// only ever looks at the one neighbour that could pair with it.
fn quote_utf16(payload: &[u8]) -> *mut u8 {
    let unit = |i: usize| u16::from_le_bytes([payload[2 * i], payload[2 * i + 1]]);
    let n = payload.len() / 2;
    let mut out_units: u32 = 2;
    for i in 0..n {
        out_units += match unit(i) {
            0x22 | 0x5C | 0x0A | 0x0D | 0x09 | 0x08 | 0x0C => 2,
            u if u < 0x20 => 6,
            u if (0xD800..=0xDFFF).contains(&u) && !surrogate_is_paired(unit, n, i) => 6,
            _ => 1,
        };
    }
    let mut block = StrBlock::alloc_with_encoding(out_units, false);
    // SAFETY: block was just allocated with 2×out_units payload bytes.
    let dst = unsafe { block.as_bytes_mut(out_units * 2) };
    let mut cur = 0usize;
    let put = |dst: &mut [u8], cur: &mut usize, u: u16| {
        dst[*cur..*cur + 2].copy_from_slice(&u.to_le_bytes());
        *cur += 2;
    };
    put(dst, &mut cur, b'"' as u16);
    for i in 0..n {
        let u = unit(i);
        let esc: Option<u8> = match u {
            0x22 => Some(b'"'),
            0x5C => Some(b'\\'),
            0x0A => Some(b'n'),
            0x0D => Some(b'r'),
            0x09 => Some(b't'),
            0x08 => Some(b'b'),
            0x0C => Some(b'f'),
            _ => None,
        };
        if let Some(e) = esc {
            put(dst, &mut cur, b'\\' as u16);
            put(dst, &mut cur, e as u16);
        } else if u < 0x20 || ((0xD800..=0xDFFF).contains(&u) && !surrogate_is_paired(unit, n, i)) {
            put(dst, &mut cur, b'\\' as u16);
            put(dst, &mut cur, b'u' as u16);
            put(dst, &mut cur, HEX[(u >> 12) as usize] as u16);
            put(dst, &mut cur, HEX[((u >> 8) & 0xf) as usize] as u16);
            put(dst, &mut cur, HEX[((u >> 4) & 0xf) as usize] as u16);
            put(dst, &mut cur, HEX[(u & 0xf) as usize] as u16);
        } else {
            put(dst, &mut cur, u);
        }
    }
    put(dst, &mut cur, b'"' as u16);
    block.into_raw()
}

/// True iff `s` contains any byte that JSON.stringify must escape:
/// the quote `"`, the backslash `\`, or any control byte `< 0x20`.
/// All other bytes (including UTF-8 continuation / lead bytes
/// `>= 0x80`) pass through unchanged. V0.2 P14-S4 — gates the
/// single-pass ASCII fast path in `__torajs_json_quote_str`.
#[inline]
fn needs_escape(s: &[u8]) -> bool {
    s.iter().any(|&c| c < 0x20 || c == b'"' || c == b'\\')
}

#[inline]
fn escaped_len(s: &[u8]) -> u32 {
    let mut out: u32 = 2; // surrounding quotes
    for &c in s {
        out += match c {
            b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 0x08 | 0x0c => 2,
            c if c < 0x20 => 6, // \uXXXX
            _ => 1,
        };
    }
    out
}

#[inline]
fn write_escaped(s: &[u8], dst: &mut [u8]) {
    dst[0] = b'"';
    let mut cur = 1usize;
    for &c in s {
        match c {
            b'"' => {
                dst[cur] = b'\\';
                dst[cur + 1] = b'"';
                cur += 2;
            }
            b'\\' => {
                dst[cur] = b'\\';
                dst[cur + 1] = b'\\';
                cur += 2;
            }
            b'\n' => {
                dst[cur] = b'\\';
                dst[cur + 1] = b'n';
                cur += 2;
            }
            b'\r' => {
                dst[cur] = b'\\';
                dst[cur + 1] = b'r';
                cur += 2;
            }
            b'\t' => {
                dst[cur] = b'\\';
                dst[cur + 1] = b't';
                cur += 2;
            }
            0x08 => {
                dst[cur] = b'\\';
                dst[cur + 1] = b'b';
                cur += 2;
            }
            0x0c => {
                dst[cur] = b'\\';
                dst[cur + 1] = b'f';
                cur += 2;
            }
            c if c < 0x20 => {
                dst[cur] = b'\\';
                dst[cur + 1] = b'u';
                dst[cur + 2] = b'0';
                dst[cur + 3] = b'0';
                dst[cur + 4] = HEX[(c >> 4) as usize];
                dst[cur + 5] = HEX[(c & 0xf) as usize];
                cur += 6;
            }
            _ => {
                dst[cur] = c;
                cur += 1;
            }
        }
    }
    dst[cur] = b'"';
}

/// `JSON.stringify(str)` — escape `s`'s payload + surround with
/// `"…"`. Returns a fresh refcount=1 Str block.
///
/// # Safety
///
/// `s` must be a valid Str heap block (layout per [`crate::layout`])
/// or NULL (nullish slot — see below).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_quote_str(s: *const u8) -> *mut u8 {
    // Nullish Str slot — the undefined sentinel (missed exec/match
    // capture) or NULL (JS null / not-yet-flipped producers). Inside
    // an array/object both stringify to `null` per ES §25.5.2 — the
    // JSON composite lane routes per-element Str through here. The
    // top-level lane goes through `__torajs_json_quote_str_top`
    // below, where the sentinel answers the undefined VALUE.
    if s.is_null() || crate::undef_sentinel::is_undef(s) {
        return unsafe { crate::literals::__torajs_null_to_str() };
    }
    let len = unsafe { (s.add(STR_LEN_OFF) as *const u32).read() };
    if !unsafe { str_is_latin1(s) } {
        // UTF-16 payload: `len` is code units, data is LE pairs —
        // the Latin-1 byte walk below would read half the payload
        // as garbage bytes. Quote in-encoding (chunk 657).
        let payload =
            unsafe { core::slice::from_raw_parts(s.add(STR_DATA_OFF), (len as usize) * 2) };
        return quote_utf16(payload);
    }
    let bytes = unsafe { core::slice::from_raw_parts(s.add(STR_DATA_OFF), len as usize) };
    // V0.2 P14-S4 — single-pass fast path for strings that need no
    // escape (common in `JSON.stringify` of user data: field names,
    // tag-like values, ASCII identifiers). When `needs_escape`
    // returns false, the output length is exactly `len + 2`
    // (surrounding quotes) — skip the second-pass `escaped_len`
    // scan, alloc the result block, write `"`, memcpy the payload,
    // write `"`. Bytes ≥ 0x20 that aren't `"` or `\` (incl. UTF-8
    // continuation / lead bytes) pass through unchanged in both
    // the fast and slow paths, so the fast-path classification
    // matches `write_escaped`'s identity arm bit-for-bit.
    if !needs_escape(bytes) {
        let out_len = len + 2;
        let mut block = StrBlock::alloc(out_len);
        // SAFETY: block was just allocated with payload capacity `out_len`.
        let dst = unsafe { block.as_bytes_mut(out_len) };
        dst[0] = b'"';
        let mid = 1 + len as usize;
        dst[1..mid].copy_from_slice(bytes);
        dst[mid] = b'"';
        return block.into_raw();
    }
    let out_len = escaped_len(bytes);
    let mut block = StrBlock::alloc(out_len);
    // SAFETY: block was just allocated with payload capacity `out_len`.
    let dst = unsafe { block.as_bytes_mut(out_len) };
    write_escaped(bytes, dst);
    block.into_raw()
}

/// Runtime `,` separator for the JSON object str_concat slow lane
/// (chunk 658 — undefined fields skip their key, so the separator
/// decision moves from compile-time `i > 0` to "has any field been
/// emitted", observable as `acc` longer than the opening `{`).
///
/// Owned-in, owned-out (642-ledger leak account): the caller hands
/// over its accumulator stake. The length ≤ 1 arm transfers `acc`
/// straight back; the concat arm releases the consumed input (a
/// first-field `{` literal is a FLAG_STATIC_LITERAL no-op) and
/// answers the fresh `acc + ","`.
///
/// # Safety
///
/// `acc` is a live Str block whose stake the caller relinquishes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_obj_sep(acc: *mut u8) -> *mut u8 {
    let len = unsafe { (acc.add(STR_LEN_OFF) as *const u32).read() };
    if len <= 1 {
        return acc;
    }
    let comma = unsafe { crate::block::__torajs_str_alloc(b",".as_ptr(), 1) };
    let out = unsafe { crate::concat::__torajs_str_concat(acc, comma) };
    unsafe { crate::__torajs_str_drop(comma) };
    unsafe { crate::__torajs_str_drop(acc) };
    out
}

/// ES §25.5.2.1 — one indent step for the static unfold: a newline
/// followed by `depth` copies of `gap`, in the gap's own encoding (a
/// UTF-16 gap makes a UTF-16 indent; the pre-560 form copied `len`
/// payload bytes into a Latin-1 block, half of such a gap). The
/// compile-time walk only emits a call to this when the call site
/// actually carries a `space` argument, so the compact form pays
/// nothing.
///
/// # Safety
///
/// `gap` is a live Str block (or NULL for the empty gap).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_indent(gap: *const u8, depth: i64) -> *mut u8 {
    let (payload, latin1) = if gap.is_null() {
        (&[][..], true)
    } else {
        unsafe { resolve_payload(gap) }
    };
    if payload.is_empty() {
        // An empty gap is the compact form (a Number space of 0, an
        // empty String space): §25.5.2.4 puts no line break in at
        // all, so this is the empty string rather than a bare "\n".
        return StrBlock::alloc(0).into_raw();
    }
    let stride = if latin1 { 1 } else { 2 };
    let depth = depth.max(0) as usize;
    let out_units = 1 + (payload.len() / stride) * depth;
    let mut block = StrBlock::alloc_with_encoding(out_units as u32, latin1);
    // SAFETY: block was just allocated with `out_units` units of capacity.
    let dst = unsafe { block.as_bytes_mut((out_units * stride) as u32) };
    dst[..stride].copy_from_slice(&[b'\n', 0][..stride]);
    for i in 0..depth {
        let at = stride + i * payload.len();
        dst[at..at + payload.len()].copy_from_slice(payload);
    }
    block.into_raw()
}

/// The `:` between a property key and its value — ES §25.5.2.4 step
/// 9.b.iii gives it a trailing space only under a NON-EMPTY gap, and
/// whether the gap is empty is a runtime fact (a Number space of 0
/// normalizes to one), so the static unfold asks here instead of
/// interning one of the two spellings at compile time.
///
/// # Safety
///
/// `gap` is a live Str block (or NULL for the empty gap).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_colon(gap: *const u8) -> *mut u8 {
    let gap_len = if gap.is_null() {
        0
    } else {
        unsafe { (gap.add(STR_LEN_OFF) as *const u32).read() }
    };
    let text: &[u8] = if gap_len == 0 { b":" } else { b": " };
    let mut block = StrBlock::alloc(text.len() as u32);
    // SAFETY: block was just allocated with `text.len()` payload bytes.
    let dst = unsafe { block.as_bytes_mut(text.len() as u32) };
    dst.copy_from_slice(text);
    block.into_raw()
}

/// The closing-bracket indent of a composite under a gap: the
/// bracket returns to the PARENT's level, but only when the body
/// emitted something — `[]` and `{}` stay on one line. "Emitted
/// something" is the same accumulator-length test
/// [`__torajs_json_obj_sep`] uses, since `acc` still holds just its
/// opening bracket otherwise.
///
/// Owned-in, owned-out (642-ledger): the caller hands over its
/// accumulator stake and receives one back.
///
/// # Safety
///
/// `acc` is a live Str block whose stake the caller relinquishes;
/// `gap` is a live Str block (or NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_close_indent(
    acc: *mut u8,
    gap: *const u8,
    depth: i64,
) -> *mut u8 {
    let gap_len = if gap.is_null() {
        0
    } else {
        unsafe { (gap.add(STR_LEN_OFF) as *const u32).read() }
    };
    let len = unsafe { (acc.add(STR_LEN_OFF) as *const u32).read() };
    if len <= 1 || gap_len == 0 {
        return acc;
    }
    let indent = unsafe { __torajs_json_indent(gap, depth) };
    let out = unsafe { crate::concat::__torajs_str_concat(acc, indent) };
    unsafe { crate::__torajs_str_drop(indent) };
    unsafe { crate::__torajs_str_drop(acc) };
    out
}

/// Top-level `JSON.stringify(str-slot)` — the undefined sentinel
/// answers the undefined VALUE itself (ES §25.5.1 step 12:
/// SerializeJSONProperty absent → stringify returns undefined),
/// unlike the composite per-element lane where undefined stringifies
/// to `null` (§25.5.2.4 step 8.b / 9.b). NULL (JS null) still
/// delegates to the `"null"` arm — `JSON.stringify(null)` IS the
/// string `null`. Everything else is the plain quote helper.
///
/// # Safety
///
/// Same contract as [`__torajs_json_quote_str`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_quote_str_top(s: *const u8) -> *mut u8 {
    if crate::undef_sentinel::is_undef(s) {
        return crate::undef_sentinel::undef_ptr();
    }
    unsafe { __torajs_json_quote_str(s) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn empty_payload_quotes() {
        assert_eq!(escaped_len(b""), 2);
        let mut buf = vec![0u8; 2];
        write_escaped(b"", &mut buf);
        assert_eq!(&buf, b"\"\"");
    }

    #[test]
    fn ascii_passthrough() {
        assert_eq!(escaped_len(b"hello"), 7);
        let mut buf = vec![0u8; 7];
        write_escaped(b"hello", &mut buf);
        assert_eq!(&buf, b"\"hello\"");
    }

    #[test]
    fn quote_and_backslash() {
        let input = br#"a"b\c"#;
        assert_eq!(escaped_len(input), 9);
        let mut buf = vec![0u8; 9];
        write_escaped(input, &mut buf);
        assert_eq!(&buf, br#""a\"b\\c""#);
    }

    #[test]
    fn whitespace_escapes() {
        let input = b"\n\r\t\x08\x0c";
        assert_eq!(escaped_len(input), 12);
        let mut buf = vec![0u8; 12];
        write_escaped(input, &mut buf);
        assert_eq!(&buf, br#""\n\r\t\b\f""#);
    }

    #[test]
    fn control_byte_unicode_escape() {
        let input = b"\x01\x1f";
        assert_eq!(escaped_len(input), 14);
        let mut buf = vec![0u8; 14];
        write_escaped(input, &mut buf);
        // Expected literal 14 bytes: " \ u 0 0 0 1 \ u 0 0 1 f "
        let expected: [u8; 14] = *b"\"\\u0001\\u001f\"";
        assert_eq!(&buf[..], &expected[..]);
    }

    #[test]
    fn high_byte_passthrough() {
        // UTF-8 bytes ≥ 0x80 pass through unchanged — JSON.stringify
        // is spec'd to escape lone surrogates but that wedge is
        // deferred; matches pre-port C behavior.
        let input = b"\xe4\xb8\xad"; // "中" in UTF-8
        assert_eq!(escaped_len(input), 5);
        let mut buf = vec![0u8; 5];
        write_escaped(input, &mut buf);
        assert_eq!(&buf, b"\"\xe4\xb8\xad\"");
    }
}
