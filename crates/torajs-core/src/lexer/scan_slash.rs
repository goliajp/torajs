//! `/` dispatch — line comment, block comment, regex literal, or
//! division. Extracted verbatim from `tokenize`'s `b'/'` match arm
//! (2026-07-03, fn-debt decomp; only change is `i` threading through
//! `&mut u32`).

use super::scan_ident::cp_continues_ident;
use super::util::{advance, decode_utf8, emit, line_terminator_at, peek, regex_context};
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
            // Line comment — consume to end-of-line / EOF. ES2024
            // §12.4 SingleLineCommentChars exclude LineTerminator.
            // Don't consume the terminator itself — outer loop's
            // whitespace branch handles it (including \r\n pairs).
            *i += 2;
            while *i < len && line_terminator_at(bytes, *i as usize).is_none() {
                *i += 1;
            }
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
            let line_break = || {
                Err(format!(
                    "unterminated regex literal at {start} (line break before closing `/`)"
                ))
            };
            loop {
                if p >= len as usize {
                    return Err(format!("unterminated regex literal starting at {start}"));
                }
                // §12.9.5 RegularExpressionNonTerminator :: SourceCharacter
                // but not LineTerminator. Every character of the body is
                // one, including the one after the `\` of a
                // RegularExpressionBackslashSequence — so a line break
                // ends the literal wherever it appears, and the escape
                // may not swallow it.
                if line_terminator_at(bytes, p).is_some() {
                    return line_break();
                }
                let c = bytes[p];
                if c == b'\\' {
                    if p + 1 >= len as usize {
                        return Err(format!("unterminated regex literal starting at {start}"));
                    }
                    if line_terminator_at(bytes, p + 1).is_some() {
                        return line_break();
                    }
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
            // §12.9.5 RegularExpressionFlags :: RegularExpressionFlags
            // IdentifierPart — the whole IdentifierPart run, not just the
            // letters. Taking the run is what makes `/a/1` one literal
            // carrying the invalid flag `1` (rejected downstream by
            // `parse_flags`) instead of a regex followed by a stray
            // number that runs fine.
            let flags_start = p + 1;
            let mut q = flags_start;
            while q < len as usize {
                // The one IdentifierPart spelling that is an early error
                // rather than a bad flag: "It is a Syntax Error if
                // IdentifierPart contains a Unicode escape sequence."
                if bytes[q] == b'\\' {
                    return Err(format!(
                        "regex literal at {start}: flags may not contain a unicode escape"
                    ));
                }
                let Some((cp, w)) = decode_utf8(bytes, q as u32) else {
                    break;
                };
                if !cp_continues_ident(cp) {
                    break;
                }
                q += w as usize;
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
