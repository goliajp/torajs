//! Lexer — TS-shaped token stream. Subset for P0.2 (just enough for
//! `console.log("hello")`). The big match-arm tokenize loop lives in
//! this file; public types in `lexer/types.rs`, scanning primitives
//! (`advance` / `peek` / `regex_context` / `emit` / ...) in
//! `lexer/util.rs`.

mod scan;
mod scan_ident;
mod scan_number;
mod scan_slash;
mod scan_template;
mod types;
mod util;

pub use types::{Span, Spanned, TemplatePart, Token};

use util::{advance, emit, peek};

pub fn tokenize(src: &str) -> Result<Vec<Spanned>, String> {
    tokenize_goal(src, false)
}

/// Script-goal token stream — annexB §B.1.3 HTML-like comments
/// (`<!--` anywhere on a line, `-->` when nothing but whitespace or
/// comments precedes it on its line) are comments under this goal and
/// nowhere else. tr's main source stays on the module-shaped
/// `tokenize` above; the eval / dynamic-Function channel parses
/// script code and enters here.
pub fn tokenize_script(src: &str) -> Result<Vec<Spanned>, String> {
    tokenize_goal(src, true)
}

fn tokenize_goal(src: &str, html_comments: bool) -> Result<Vec<Spanned>, String> {
    let bytes = src.as_bytes();
    let len = bytes.len() as u32;
    let mut out = Vec::new();
    let mut i: u32 = 0;
    // §B.1.3 — whether the current line has produced a token yet. A
    // `-->` close-comment is only a comment when it hasn't: the
    // grammar puts it after a LineTerminator (or a multi-line comment
    // that contains one), never mid-expression, which keeps `a --> b`
    // meaning postfix-decrement-then-greater.
    let mut fresh_line = true;

    // ES2023 §12.5 HashbangComment — `#!` runs to the end of the line and
    // is permitted only at the very start of the source text, so this is
    // deliberately outside the loop rather than a `#` match arm. A `#`
    // anywhere else is still either a private name or an error.
    if bytes.starts_with(b"#!") {
        while i < len && bytes[i as usize] != b'\n' && bytes[i as usize] != b'\r' {
            i += 1;
        }
    }

    while i < len {
        let start = i;
        let b = bytes[i as usize];
        // The non-ASCII half of §12.2 WhiteSpace / §12.3 LineTerminator:
        // <NBSP>, <ZWNBSP>, the Zs category, and U+2028 / U+2029.
        // Terminators are skipped as whitespace here so single-line
        // comments and ASI still observe them. A non-ASCII character that
        // is not whitespace falls through to the identifier arm below,
        // and from there to the unexpected-byte error.
        if b >= 0x80
            && let Some((cp, w)) = util::decode_utf8(bytes, i)
            && is_whitespace_cp(cp)
        {
            if cp == 0x2028 || cp == 0x2029 {
                fresh_line = true;
            }
            i += w;
            continue;
        }
        let toks_before = out.len();
        match b {
            // ES2024 §12.2 WhiteSpace + §12.3 LineTerminator, ASCII half:
            // <TAB> <VT> <FF> <SP> <LF> <CR>.
            b' ' | b'\t' | 0x0B | 0x0C | b'\r' | b'\n' => {
                if b == b'\r' || b == b'\n' {
                    fresh_line = true;
                }
                i += 1;
                continue;
            }
            // §B.1.3 SingleLineHTMLOpenComment — `<!--` starts a
            // comment anywhere (script goal only), running to
            // end-of-line like `//`.
            b'<' if html_comments && bytes[i as usize..].starts_with(b"<!--") => {
                skip_single_line_html(bytes, &mut i, len);
                continue;
            }
            // §B.1.3 SingleLineHTMLCloseComment — `-->` on a line
            // that has produced no token yet (script goal only).
            b'-' if html_comments && fresh_line && bytes[i as usize..].starts_with(b"-->") => {
                skip_single_line_html(bytes, &mut i, len);
                continue;
            }
            b'.' => scan_number::scan_dot(bytes, &mut i, &mut out, start),
            b',' => emit(&mut out, Token::Comma, start, advance(&mut i)),
            b':' => emit(&mut out, Token::Colon, start, advance(&mut i)),
            b';' => emit(&mut out, Token::Semi, start, advance(&mut i)),
            b'(' => emit(&mut out, Token::LParen, start, advance(&mut i)),
            b')' => emit(&mut out, Token::RParen, start, advance(&mut i)),
            b'{' => emit(&mut out, Token::LBrace, start, advance(&mut i)),
            b'}' => emit(&mut out, Token::RBrace, start, advance(&mut i)),
            b'[' => emit(&mut out, Token::LBracket, start, advance(&mut i)),
            b']' => emit(&mut out, Token::RBracket, start, advance(&mut i)),
            b'+' => {
                i += 1;
                if peek(bytes, i) == Some(b'+') {
                    i += 1;
                    emit(&mut out, Token::PlusPlus, start, i);
                } else if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    emit(&mut out, Token::PlusEq, start, i);
                } else {
                    emit(&mut out, Token::Plus, start, i);
                }
            }
            b'-' => {
                i += 1;
                if peek(bytes, i) == Some(b'-') {
                    i += 1;
                    emit(&mut out, Token::MinusMinus, start, i);
                } else if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    emit(&mut out, Token::MinusEq, start, i);
                } else {
                    emit(&mut out, Token::Minus, start, i);
                }
            }
            b'*' => {
                i += 1;
                /* V3-01 — `**` exponent operator (and its compound
                 * assign `**=`). JS spec: right-associative,
                 * precedence higher than mul / div / mod. */
                if peek(bytes, i) == Some(b'*') {
                    i += 1;
                    if peek(bytes, i) == Some(b'=') {
                        i += 1;
                        emit(&mut out, Token::StarStarEq, start, i);
                    } else {
                        emit(&mut out, Token::StarStar, start, i);
                    }
                } else if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    emit(&mut out, Token::StarEq, start, i);
                } else {
                    emit(&mut out, Token::Star, start, i);
                }
            }
            b'~' => emit(&mut out, Token::Tilde, start, advance(&mut i)),
            b'?' => {
                // `?` (ternary), `??` (nullish coalescing), `?.`
                // (optional chaining). Single-char emit becomes
                // multi-char when the suffix is `?` or `.`.
                if peek(bytes, i + 1) == Some(b'?') {
                    i += 2;
                    emit(&mut out, Token::QuestionQuestion, start, i);
                } else if peek(bytes, i + 1) == Some(b'.') {
                    i += 2;
                    emit(&mut out, Token::QuestionDot, start, i);
                } else {
                    emit(&mut out, Token::Question, start, advance(&mut i));
                }
            }
            b'/' => scan_slash::scan_slash(bytes, &mut i, &mut out, start, len)?,
            b'%' => {
                i += 1;
                if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    emit(&mut out, Token::PercentEq, start, i);
                } else {
                    emit(&mut out, Token::Percent, start, i);
                }
            }
            b'&' => {
                i += 1;
                if peek(bytes, i) == Some(b'&') {
                    i += 1;
                    emit(&mut out, Token::AmpAmp, start, i);
                } else {
                    emit(&mut out, Token::Amp, start, i);
                }
            }
            b'|' => {
                i += 1;
                if peek(bytes, i) == Some(b'|') {
                    i += 1;
                    emit(&mut out, Token::PipePipe, start, i);
                } else {
                    emit(&mut out, Token::Pipe, start, i);
                }
            }
            b'^' => emit(&mut out, Token::Caret, start, advance(&mut i)),
            b'<' => {
                i += 1;
                if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    emit(&mut out, Token::LtEq, start, i);
                } else if peek(bytes, i) == Some(b'<') {
                    i += 1;
                    emit(&mut out, Token::ShlShl, start, i);
                } else {
                    emit(&mut out, Token::Lt, start, i);
                }
            }
            b'>' => scan_gt(bytes, &mut i, &mut out, start),
            b'=' => scan_eq(bytes, &mut i, &mut out, start),
            b'!' => scan_bang(bytes, &mut i, &mut out, start),
            b'"' | b'\'' => scan::scan_string(bytes, &mut i, &mut out, start, len)?,
            b'`' => scan_template::scan_template(bytes, &mut i, &mut out, start, len)?,
            b'#' if scan_ident::ident_start_at(bytes, i + 1) => {
                // P8.1 — `#name` PrivateIdentifier; a bare `#` not
                // followed by an ident start falls through to the
                // unexpected-byte error below.
                scan_ident::scan_private_ident(bytes, &mut i, &mut out, start, len)?
            }
            b if b.is_ascii_digit() => {
                scan_number::scan_number(bytes, &mut i, &mut out, start, len, b)?
            }
            // ES §12.7.1 IdentifierStart. Kept below the digit arm so
            // `0`-`9` still route to the numeric scanner; a non-ASCII
            // character or a `\u` escape gets here only after every ASCII
            // byte arm has declined it.
            _ if scan_ident::ident_start_at(bytes, i) => {
                scan_ident::scan_ident_or_keyword(bytes, &mut i, &mut out, start, len)?
            }
            _ => return Err(format!("unexpected byte {b:#x} at {start}")),
        }
        // A token landed → the line is no longer fresh. A comment
        // landed nothing; it keeps the line fresh only if it spans a
        // line break (§B.1.3 admits `-->` after a multi-line comment
        // containing one) or leaves the current line's state alone.
        if out.len() != toks_before {
            fresh_line = false;
        } else if has_line_break(&bytes[start as usize..i as usize]) {
            fresh_line = true;
        }
    }
    emit(&mut out, Token::Eof, len, len);
    Ok(out)
}

/// Consume an HTML-like comment opener and everything to end-of-line;
/// the terminator itself stays for the whitespace arm (same posture as
/// `scan_slash`'s line comment).
fn skip_single_line_html(bytes: &[u8], i: &mut u32, len: u32) {
    while *i < len {
        let c = bytes[*i as usize];
        if c == b'\n' || c == b'\r' {
            break;
        }
        if c == 0xE2
            && (*i as usize) + 2 < len as usize
            && bytes[*i as usize + 1] == 0x80
            && (bytes[*i as usize + 2] == 0xA8 || bytes[*i as usize + 2] == 0xA9)
        {
            break;
        }
        *i += 1;
    }
}

/// Whether a byte slice contains any §12.3 LineTerminator — LF, CR, or
/// the UTF-8 spellings of U+2028 / U+2029.
fn has_line_break(bytes: &[u8]) -> bool {
    bytes.iter().enumerate().any(|(k, &c)| {
        c == b'\n'
            || c == b'\r'
            || (c == 0xE2
                && bytes.get(k + 1) == Some(&0x80)
                && matches!(bytes.get(k + 2), Some(&0xA8) | Some(&0xA9)))
    })
}

/// The non-ASCII members of ES2024 §12.2 `WhiteSpace` — <NBSP>,
/// <ZWNBSP>, and the Zs category — plus the two §12.3 `LineTerminator`
/// characters U+2028 / U+2029, which the caller skips the same way.
///
/// Zs is spelled out rather than looked up: it is eight ranges that have
/// not moved in a decade, and pulling a UCD table in for it would cost
/// more than it explains.
fn is_whitespace_cp(cp: u32) -> bool {
    matches!(
        cp,
        0x00A0                  // NBSP
        | 0x1680                // OGHAM SPACE MARK
        | 0x2000
            ..=0x200A       // EN QUAD .. HAIR SPACE
        | 0x2028 | 0x2029       // LINE / PARAGRAPH SEPARATOR
        | 0x202F                // NARROW NO-BREAK SPACE
        | 0x205F                // MEDIUM MATHEMATICAL SPACE
        | 0x3000                // IDEOGRAPHIC SPACE
        | 0xFEFF // ZWNBSP
    )
}

/// `>` / `>=` / `>>` / `>>>` — greater-than family. Right-shift arms
/// (`>>` / `>>>`) require an extra peek; `>=` and bare `>` are the
/// same shape as `+=` / `+`.
fn scan_gt(bytes: &[u8], i: &mut u32, out: &mut Vec<Spanned>, start: u32) {
    *i += 1;
    if peek(bytes, *i) == Some(b'=') {
        *i += 1;
        emit(out, Token::GtEq, start, *i);
    } else if peek(bytes, *i) == Some(b'>') {
        *i += 1;
        if peek(bytes, *i) == Some(b'>') {
            *i += 1;
            emit(out, Token::ShrShrShr, start, *i);
        } else {
            emit(out, Token::ShrShr, start, *i);
        }
    } else {
        emit(out, Token::Gt, start, *i);
    }
}

/// `=` / `==` / `===` / `=>` — assignment / equality / arrow. Three-
/// wide arms (`===`) require an extra peek; the fat-arrow arm (`=>`)
/// keeps parser-side arrow-function detection off the operator hot
/// path.
fn scan_eq(bytes: &[u8], i: &mut u32, out: &mut Vec<Spanned>, start: u32) {
    *i += 1;
    if peek(bytes, *i) == Some(b'=') {
        *i += 1;
        if peek(bytes, *i) == Some(b'=') {
            *i += 1;
            emit(out, Token::EqEqEq, start, *i);
        } else {
            // V3-18 m3 — `==` IsLooselyEqual per §7.2.13. Restored
            // from "out-of-scope" 2026-05-10 (test262 100% bar).
            // Emits a new Token::EqEq → BinOp::LooseEq.
            emit(out, Token::EqEq, start, *i);
        }
    } else if peek(bytes, *i) == Some(b'>') {
        *i += 1;
        emit(out, Token::FatArrow, start, *i);
    } else {
        emit(out, Token::Eq, start, *i);
    }
}

/// `!` / `!=` / `!==` — logical not + non-equality.
fn scan_bang(bytes: &[u8], i: &mut u32, out: &mut Vec<Spanned>, start: u32) {
    *i += 1;
    if peek(bytes, *i) == Some(b'=') {
        *i += 1;
        if peek(bytes, *i) == Some(b'=') {
            *i += 1;
            emit(out, Token::BangEqEq, start, *i);
        } else {
            // V3-18 m3 — `!=` is `!IsLooselyEqual`.
            emit(out, Token::BangEq, start, *i);
        }
    } else {
        // Unary logical not — used as `!cond`. M1.5.
        emit(out, Token::Bang, start, *i);
    }
}
