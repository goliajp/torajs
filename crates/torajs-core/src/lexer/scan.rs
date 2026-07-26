//! Per-token-category lexer subroutines extracted from the
//! `tokenize` match-arm dispatcher in `lexer.rs`. Each scanner
//! advances `i` past the consumed bytes and pushes the produced
//! `Spanned` onto `out`.
//!
//! - `scan_string` — `"…"` and `'…'` literals with full JS-spec
//!   escape decoding (`\n` / `\xNN` / `\uNNNN` / `\u{N…N}` / …).
//!   Returns `Err` on unterminated literals.
//! - `scan_ident_or_keyword` — ident-start byte through to the next
//!   non-ident byte; emits the keyword token if the slice matches
//!   the reserved-word table, else `Token::Ident(name)`.
//! - `scan_number` — every numeric literal shape: decimal, BigInt,
//!   leading-dot, binary (`0b`), octal (`0o`), hex (`0x`). Returns
//!   `Err` on empty digit groups (e.g. `0b`).
//!
//! Extracted from `lexer.rs` (2026-05-25, god-file decomp batch 23).

use super::util::{decode_utf8, emit, is_ident_cont, is_ident_cont_cp, push_codepoint};
use super::{Spanned, Token};

/// Walk `i` forward over `IdentifierPart` characters (ES §12.7.1).
/// ASCII stays a plain byte step; a byte ≥ 0x80 is decoded once and
/// admitted only if it carries `ID_Continue` (or is ZWNJ / ZWJ, which
/// the table folds in). Anything else stops the walk without consuming,
/// so the caller's span ends on a codepoint boundary.
fn walk_ident_part(bytes: &[u8], i: &mut u32, len: u32) {
    while *i < len {
        let b = bytes[*i as usize];
        if is_ident_cont(b) {
            *i += 1;
        } else if b >= 0x80 {
            match decode_utf8(bytes, *i) {
                Some((cp, w)) if is_ident_cont_cp(cp) => *i += w,
                _ => break,
            }
        } else {
            break;
        }
    }
}

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
                    // §12.9.4.2 SV — a high-surrogate escape directly
                    // followed by a low-surrogate escape is ONE
                    // supplementary code point (UTF-16 string value
                    // semantics; the internal UTF-8 buffer can't hold
                    // the halves separately — each alone became U+FFFD
                    // and every `"𝒢"`-spelled literal was
                    // destroyed). Lone surrogates still fall back to
                    // U+FFFD in push_codepoint (WTF-8 residual).
                    if (0xD800..=0xDBFF).contains(&cp)
                        && *i + 11 < len
                        && bytes[*i as usize + 6] == b'\\'
                        && bytes[*i as usize + 7] == b'u'
                        && bytes[*i as usize + 8].is_ascii_hexdigit()
                        && bytes[*i as usize + 9].is_ascii_hexdigit()
                        && bytes[*i as usize + 10].is_ascii_hexdigit()
                        && bytes[*i as usize + 11].is_ascii_hexdigit()
                    {
                        let mut lo: u32 = 0;
                        for k in 8..=11 {
                            lo = lo * 16 + (bytes[*i as usize + k] as char).to_digit(16).unwrap();
                        }
                        if (0xDC00..=0xDFFF).contains(&lo) {
                            let combined = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                            push_codepoint(&mut buf, combined);
                            *i += 12;
                            continue;
                        }
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
    let value =
        String::from_utf8(buf).map_err(|_| format!("invalid utf-8 in string at {start}"))?;
    *i += 1; // consume closing quote
    emit(out, Token::String(value), start, *i);
    Ok(())
}

pub(super) fn scan_ident_or_keyword(
    bytes: &[u8],
    i: &mut u32,
    out: &mut Vec<Spanned>,
    start: u32,
    len: u32,
) {
    walk_ident_part(bytes, i, len);
    let name = std::str::from_utf8(&bytes[start as usize..*i as usize])
        .expect("ident slice ends on a codepoint boundary");
    let token = match name {
        "let" => Token::Let,
        "const" => Token::Const,
        // V3-18 m4 first wedge — `var` lexes as Let.
        // Full hoisting + function-scope semantics
        // (vs let/const block-scope) is a follow-up;
        // many test262 cases use `var` for plain
        // top-level declarations and just need it to
        // parse + behave like let. Programs that depend
        // on hoisting to use `var` before its decl will
        // continue to fail until the m4.b hoisting pass.
        "var" => Token::Var,
        "if" => Token::If,
        "else" => Token::Else,
        "true" => Token::True,
        "false" => Token::False,
        "while" => Token::While,
        "for" => Token::For,
        "break" => Token::Break,
        "continue" => Token::Continue,
        "function" => Token::Function,
        "return" => Token::Return,
        "type" => Token::Type,
        "try" => Token::Try,
        "catch" => Token::Catch,
        "finally" => Token::Finally,
        "throw" => Token::Throw,
        "class" => Token::Class,
        "new" => Token::New,
        "this" => Token::This,
        "extends" => Token::Extends,
        "super" => Token::Super,
        "do" => Token::Do,
        "switch" => Token::Switch,
        "case" => Token::Case,
        "default" => Token::Default,
        "typeof" => Token::TypeOf,
        "delete" => Token::Delete,
        "void" => Token::Void,
        "instanceof" => Token::InstanceOf,
        "yield" => Token::Yield,
        "async" => Token::Async,
        "await" => Token::Await,
        "import" => Token::Import,
        "export" => Token::Export,
        // `from` and `as` are contextual keywords in TS —
        // they may appear as plain identifiers outside
        // import context (`let from = 1` is legal). Lexer
        // keeps them as Ident; parser recognizes them by
        // string match in the import-decl tail.
        "null" => Token::Null,
        _ => Token::Ident(name.to_string()),
    };
    emit(out, token, start, *i);
}

/// `#name` PrivateIdentifier scanner (P8.1). Extracted verbatim
/// from `tokenize`'s `b'#'` match arm (2026-07-03, fn-debt decomp;
/// the `is_ident_start` guard stays at the call site so a bare `#`
/// still falls through to the unexpected-byte error).
pub(super) fn scan_private_ident(
    bytes: &[u8],
    i: &mut u32,
    out: &mut Vec<Spanned>,
    start: u32,
    len: u32,
) {
    *i += 1;
    let ident_start = *i;
    walk_ident_part(bytes, i, len);
    let name = std::str::from_utf8(&bytes[ident_start as usize..*i as usize])
        .expect("ident slice ends on a codepoint boundary");
    emit(out, Token::PrivateIdent(name.to_string()), start, *i);
}
