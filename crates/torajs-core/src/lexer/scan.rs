//! Per-token-category lexer subroutines extracted from the
//! `tokenize` match-arm dispatcher in `lexer.rs`. Each scanner
//! advances `i` past the consumed bytes and pushes the produced
//! `Spanned` onto `out`.
//!
//! - `scan_string` — `"…"` and `'…'` literals with full JS-spec
//!   escape decoding (`\n` / `\xNN` / `\uNNNN` / `\u{N…N}` / …).
//!   Returns `Err` on unterminated literals, on a raw LF / CR in the
//!   body, and on a malformed `\x` / `\u` — none of those has a
//!   passthrough arm in §12.9.4.
//! - `scan_number` — every numeric literal shape: decimal, BigInt,
//!   leading-dot, binary (`0b`), octal (`0o`), hex (`0x`). Returns
//!   `Err` on empty digit groups (e.g. `0b`).
//!
//! Identifier and private-name scanning lives next door in
//! `scan_ident.rs` — it grew an escape decoder and a reserved-word rule
//! of its own.
//!
//! Extracted from `lexer.rs` (2026-05-25, god-file decomp batch 23).

use super::util::{emit, push_codepoint, scan_hex_escape};
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
            // `\xNN` / `\uNNNN` / `\u{N…N}`. §12.9.4 EscapeCharacter
            // lists `x` and `u`, so neither is a NonEscapeCharacter: a
            // malformed spelling has no passthrough arm to fall into —
            // it is a SyntaxError, not a literal `x` / `u`.
            if matches!(esc, b'x' | b'u') {
                let Some((cp, n)) = scan_hex_escape(bytes, *i) else {
                    return Err(format!(
                        "malformed \\{} escape in string at {start}",
                        esc as char
                    ));
                };
                push_codepoint(&mut buf, cp);
                *i += n;
                continue;
            }
            // annexB §B.1.2 LegacyOctalEscapeSequence — `\101` is `A`,
            // not `101`. Ahead of the `\0` arm below, which keeps only
            // the non-legacy spelling (`\0` with no digit after it).
            if let Some((cp, n)) = super::legacy_octal::scan_legacy_octal_escape(bytes, *i) {
                push_codepoint(&mut buf, cp);
                *i += n;
                continue;
            }
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
        // §12.9.4 DoubleStringCharacter :: SourceCharacter but not one
        // of " or \ or LineTerminator — with <LS> and <PS> re-admitted
        // as their own alternatives. So the rejected set is exactly the
        // LineTerminators that are not those two: LF and CR.
        if c != 0xE2 && super::util::line_terminator_at(bytes, *i as usize).is_some() {
            return Err(format!("unterminated string starting at {start}"));
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
