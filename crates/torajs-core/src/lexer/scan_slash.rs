//! `/` dispatch — line comment, block comment, regex literal, or
//! division. Extracted verbatim from `tokenize`'s `b'/'` match arm
//! (2026-07-03, fn-debt decomp; only change is `i` threading through
//! `&mut u32`).

use super::util::{advance, emit, peek, regex_context};
use super::{Spanned, Token};

pub(super) fn scan_slash(
    bytes: &[u8],
    i: &mut u32,
    out: &mut Vec<Spanned>,
    start: u32,
    len: u32,
) -> Result<(), String> {
    // `//` line comment, `/* */` block comment, regex
    // literal, or division. Disambiguation between regex
    // and division uses the previous token: regex when prev
    // is None / a punctuator / a keyword that can start an
    // expression on its right.
    match peek(bytes, *i + 1) {
        Some(b'/') => {
            // Line comment — consume to end-of-line / EOF.
            *i += 2;
            while *i < len && bytes[*i as usize] != b'\n' {
                *i += 1;
            }
            // Don't consume the newline itself — outer loop's
            // whitespace branch handles it (and any trailing
            // \r\n line ending).
        }
        Some(b'*') => {
            // Block comment — consume to first `*/`. Nested
            // block comments are NOT supported (TS doesn't
            // support them either; matches `tsc` / `bun`).
            *i += 2;
            let comment_start = start;
            loop {
                if *i + 1 >= len {
                    return Err(format!(
                        "unterminated block comment starting at {comment_start}"
                    ));
                }
                if bytes[*i as usize] == b'*' && bytes[(*i + 1) as usize] == b'/' {
                    *i += 2;
                    break;
                }
                *i += 1;
            }
        }
        _ if regex_context(out.last().map(|s| &s.token)) => {
            // Scan a regex literal: `/pattern/flags`.
            // Pattern body: read until an unescaped `/`,
            // honoring `\\.` escapes and `[...]` character
            // classes (where `/` is allowed bare).
            let body_start = (*i + 1) as usize;
            let mut p = body_start;
            let mut in_class = false;
            loop {
                if p >= len as usize {
                    return Err(format!("unterminated regex literal starting at {start}"));
                }
                let c = bytes[p];
                if c == b'\n' {
                    return Err(format!(
                        "unterminated regex literal at {start} (line break before closing `/`)"
                    ));
                }
                if c == b'\\' {
                    // Skip the escape sequence's next byte.
                    p += 2;
                    continue;
                }
                if c == b'[' {
                    in_class = true;
                    p += 1;
                    continue;
                }
                if c == b']' && in_class {
                    in_class = false;
                    p += 1;
                    continue;
                }
                if c == b'/' && !in_class {
                    break;
                }
                p += 1;
            }
            let pattern = String::from_utf8_lossy(&bytes[body_start..p]).into_owned();
            // Flags: any trailing ASCII letters.
            let flags_start = p + 1;
            let mut q = flags_start;
            while q < len as usize && bytes[q].is_ascii_alphabetic() {
                q += 1;
            }
            let flags = String::from_utf8_lossy(&bytes[flags_start..q]).into_owned();
            *i = q as u32;
            emit(out, Token::Regex { pattern, flags }, start, *i);
        }
        Some(b'=') => {
            *i += 2;
            emit(out, Token::SlashEq, start, *i);
        }
        _ => emit(out, Token::Slash, start, advance(i)),
    }
    Ok(())
}
