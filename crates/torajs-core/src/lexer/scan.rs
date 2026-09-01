//! Per-token-category lexer subroutines extracted from the
//! `tokenize` match-arm dispatcher in `lexer.rs`. Each scanner
//! advances `i` past the consumed bytes and pushes the produced
//! `Spanned` onto `out`.
//!
//! - `scan_string` — `"…"` and `'…'` literals with full JS-spec
//!   escape decoding (`\n` / `\xNN` / `\uNNNN` / `\u{N…N}` / …).
//!   Returns `Err` on unterminated literals.
//! - `scan_number` — every numeric literal shape: decimal, BigInt,
//!   leading-dot, binary (`0b`), octal (`0o`), hex (`0x`). Returns
//!   `Err` on empty digit groups (e.g. `0b`).
//!
//! Identifier and private-name scanning lives next door in
//! `scan_ident.rs` — it grew an escape decoder and a reserved-word rule
//! of its own.
//!
//! Extracted from `lexer.rs` (2026-05-25, god-file decomp batch 23).

use super::util::{emit, push_codepoint};
use super::{Spanned, Token};

pub(super) fn scan_string(
    bytes: &[u8],
    i: &mut u32,
    out: &mut Vec<Spanned>,
    start: u32,
    len: u32,
) -> Result<(), String> {
    let quote = bytes[*i as usize];
    *i += 1;
    // Decode JS-style escape sequences. Supported: \\ \" \'
    // \n \r \t \b \f \v \0 \xNN \uNNNN \u{NNNN...}.
    // Unknown escapes pass through their letter (matches
    // V8's annex-B-friendly behavior for the small subset
    // our tests need).
    let mut buf: Vec<u8> = Vec::new();
    while *i < len && bytes[*i as usize] != quote {
        let c = bytes[*i as usize];
        if c == b'\\' && *i + 1 < len {
            let esc = bytes[*i as usize + 1];
            match esc {
                b'n' => {
                    buf.push(b'\n');
                    *i += 2;
                    continue;
                }
                b'r' => {
                    buf.push(b'\r');
                    *i += 2;
                    continue;
                }
                b't' => {
                    buf.push(b'\t');
                    *i += 2;
                    continue;
                }
                b'b' => {
                    buf.push(0x08);
                    *i += 2;
                    continue;
                }
                b'f' => {
                    buf.push(0x0c);
                    *i += 2;
                    continue;
                }
                b'v' => {
                    buf.push(0x0b);
                    *i += 2;
                    continue;
                }
                b'0' => {
                    buf.push(0);
                    *i += 2;
                    continue;
                }
                b'\\' => {
                    buf.push(b'\\');
                    *i += 2;
                    continue;
                }
                b'\'' => {
                    buf.push(b'\'');
                    *i += 2;
                    continue;
                }
                b'"' => {
                    buf.push(b'"');
                    *i += 2;
                    continue;
                }
                b'`' => {
                    buf.push(b'`');
                    *i += 2;
                    continue;
                }
                // V3-18 m1.h.33 — `\xNN` hex escape (2 hex
                // digits → byte). Per JS spec §12.8.4.1
                // HexEscapeSequence.
                b'x' if *i + 3 < len
                    && bytes[*i as usize + 2].is_ascii_hexdigit()
                    && bytes[*i as usize + 3].is_ascii_hexdigit() =>
                {
                    let hi = (bytes[*i as usize + 2] as char).to_digit(16).unwrap();
                    let lo = (bytes[*i as usize + 3] as char).to_digit(16).unwrap();
                    let cp = (hi * 16 + lo) as u32;
                    push_codepoint(&mut buf, cp);
                    *i += 4;
                    continue;
                }
                // V3-18 m1.h.33 — `\uNNNN` 4-digit unicode
                // escape. Per JS spec §12.8.4.1 UnicodeEscapeSequence.
                b'u' if *i + 5 < len
                    && bytes[*i as usize + 2].is_ascii_hexdigit()
                    && bytes[*i as usize + 3].is_ascii_hexdigit()
                    && bytes[*i as usize + 4].is_ascii_hexdigit()
                    && bytes[*i as usize + 5].is_ascii_hexdigit() =>
                {
                    let mut cp: u32 = 0;
                    for k in 2..=5 {
                        cp = cp * 16 + (bytes[*i as usize + k] as char).to_digit(16).unwrap();
                    }
                    push_codepoint(&mut buf, cp);
                    *i += 6;
                    continue;
                }
                // `\u{N...N}` extended form (1-6 hex digits).
                // Per JS spec §12.8.4.1 LegacyOctalEscape
                // not handled; ES2015+ form only.
                b'u' if *i + 3 < len && bytes[*i as usize + 2] == b'{' => {
                    let mut k = *i as usize + 3;
                    let mut cp: u32 = 0;
                    let mut digits = 0;
                    while k < len as usize && bytes[k].is_ascii_hexdigit() && digits < 6 {
                        cp = cp * 16 + (bytes[k] as char).to_digit(16).unwrap();
                        k += 1;
                        digits += 1;
                    }
                    if digits >= 1 && k < len as usize && bytes[k] == b'}' {
                        push_codepoint(&mut buf, cp);
                        *i = (k + 1) as u32;
                        continue;
                    }
                    // malformed → fall through to passthrough
                    buf.push(esc);
                    *i += 2;
                    continue;
                }
                // §12.9.4.3 LineContinuation — `\` followed by a
                // LineTerminatorSequence contributes nothing to the
                // string value (SV is the empty sequence). `\r\n`
                // counts as one sequence.
                b'\n' => {
                    *i += 2;
                    continue;
                }
                b'\r' => {
                    *i += 2;
                    if *i < len && bytes[*i as usize] == b'\n' {
                        *i += 1;
                    }
                    continue;
                }
                // U+2028 / U+2029 (LS / PS) in UTF-8: E2 80 A8 / A9.
                0xE2 if *i + 3 < len
                    && bytes[*i as usize + 2] == 0x80
                    && (bytes[*i as usize + 3] == 0xA8 || bytes[*i as usize + 3] == 0xA9) =>
                {
                    *i += 4;
                    continue;
                }
                other => {
                    buf.push(other);
                    *i += 2;
                    continue;
                }
            }
        }
        buf.push(c);
        *i += 1;
    }
    if *i >= len {
        return Err(format!("unterminated string starting at {start}"));
    }
    let value = torajs_wtf8::Wtf8Buf::from_bytes(buf)
        .map_err(|_| format!("invalid utf-8 in string at {start}"))?;
    *i += 1; // consume closing quote
    emit(out, Token::String(value), start, *i);
    Ok(())
}
