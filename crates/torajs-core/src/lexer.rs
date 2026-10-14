//! Lexer — TS-shaped token stream. Subset for P0.2 (just enough for
//! `console.log("hello")`). The big match-arm tokenize loop lives in
//! this file; public types in `lexer/types.rs`, scanning primitives
//! (`advance` / `peek` / `regex_context` / `emit` / ...) in
//! `lexer/util.rs`.

pub(crate) mod legacy_octal;
mod scan;
mod scan_ident;
mod scan_number;
mod scan_slash;
mod scan_template;
mod types;
pub(crate) mod util;

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
    // §12.9 — which `{`s open a block (true) and which an object
    // literal, so the `}` closing one can say whether a `/` after it
    // is a regex literal; `rbrace_closed_block` is that answer for
    // the most recent `}` (read only when `}` is the previous token).
    let mut brace_kinds: Vec<bool> = Vec::new();
    let mut rbrace_closed_block = false;
    // The `(` stack's function-head entries: `Some(v)` when the `(`
    // opened a `function` parameter list, `v` = whether a `/` after
    // that function's body `}` is a regex — true for a declaration
    // (statement position), false for a function EXPRESSION
    // (`function(){} / 2` is division; test262 division/
    // S11.5.2_A3_T1.5). `last_rparen_fn` is the entry the most
    // recent `)` popped, read by the `{` right after it.
    let mut paren_fn_heads: Vec<Option<bool>> = Vec::new();
    let mut last_rparen_fn: Option<bool> = None;

    skip_hashbang(bytes, &mut i, len);

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
            b'.' => {
                scan_number::scan_dot(bytes, &mut i, &mut out, start)?;
                reject_ident_after_numeric(bytes, &out, i)?;
            }
            b',' => emit(&mut out, Token::Comma, start, advance(&mut i)),
            b':' => emit(&mut out, Token::Colon, start, advance(&mut i)),
            b';' => emit(&mut out, Token::Semi, start, advance(&mut i)),
            b'(' => {
                paren_fn_heads.push(util::function_head_regex_after(&out));
                emit(&mut out, Token::LParen, start, advance(&mut i));
            }
            b')' => {
                last_rparen_fn = paren_fn_heads.pop().flatten();
                emit(&mut out, Token::RParen, start, advance(&mut i));
            }
            b'{' => {
                let prev = out.last().map(|s| &s.token);
                // A function body: the `}` follows the function's own
                // position, not the `)` before it.
                let kind = match (prev, last_rparen_fn) {
                    (Some(Token::RParen), Some(v)) => v,
                    _ => util::block_brace_opens(prev, brace_kinds.last().copied()),
                };
                brace_kinds.push(kind);
                emit(&mut out, Token::LBrace, start, advance(&mut i));
            }
            b'}' => {
                rbrace_closed_block = brace_kinds.pop().unwrap_or(true);
                emit(&mut out, Token::RBrace, start, advance(&mut i));
            }
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
            b'/' => {
                scan_slash::scan_slash(bytes, &mut i, &mut out, start, len, rbrace_closed_block)?
            }
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
                scan_number::scan_number(bytes, &mut i, &mut out, start, len, b)?;
                reject_ident_after_numeric(bytes, &out, i)?;
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

/// ES2023 §12.5 HashbangComment — `#!` runs to the end of the line and
/// is permitted only at the very start of the source text, which is why
/// this is a prologue rather than a `#` arm of the scan loop: a `#`
/// anywhere else is a private name or an error.
///
/// §12.3 LineTerminator has four members, and this used to carry a
/// hand-written copy with two of them, so a hashbang ended by U+2028 /
/// U+2029 swallowed the first line of the program.
/// `line_terminator_at` is the one spelling, and its own doc records
/// the last time these copies were consolidated; this one was not in
/// that sweep.
fn skip_hashbang(bytes: &[u8], i: &mut u32, len: u32) {
    if !bytes.starts_with(b"#!") {
        return;
    }
    while *i < len && util::line_terminator_at(bytes, *i as usize).is_none() {
        *i += 1;
    }
}

/// §12.9.3 — "The SourceCharacter immediately following a
/// NumericLiteral must not be an IdentifierStart or DecimalDigit."
///
/// Without it the scanner takes the longest literal it can and hands
/// the rest to the identifier arm, so `123abc` becomes `123` and
/// `abc`, `0b12` becomes `0b1` and `2`, and `1.2.3` becomes `1.2` and
/// `.3` — three programs that are Syntax Errors read as three programs
/// that are not. Nothing downstream can recover the difference; by
/// then the two tokens are indistinguishable from two the source
/// really did write.
///
/// Checked at the CALL SITE rather than inside the scanners because
/// the numeric path has several exits (radix, decimal, leading-dot)
/// and `scan_dot` may not have produced a number at all — `.foo` is a
/// member access, and the last token says which happened.
fn reject_ident_after_numeric(bytes: &[u8], out: &[Spanned], i: u32) -> Result<(), String> {
    if !matches!(
        out.last().map(|s| &s.token),
        Some(Token::Number(_) | Token::BigInt { .. })
    ) {
        return Ok(());
    }
    let Some(&b) = bytes.get(i as usize) else {
        return Ok(());
    };
    if b.is_ascii_digit()
        || util::is_ident_start(b)
        || (b >= 0x80 && scan_ident::ident_start_at(bytes, i))
    {
        return Err(format!(
            "a numeric literal may not be followed directly by an identifier or digit at {i} (ES §12.9.3)"
        ));
    }
    Ok(())
}

/// Consume an HTML-like comment opener and everything to end-of-line;
/// the terminator itself stays for the whitespace arm (same posture as
/// `scan_slash`'s line comment).
fn skip_single_line_html(bytes: &[u8], i: &mut u32, len: u32) {
    while *i < len && util::line_terminator_at(bytes, *i as usize).is_none() {
        *i += 1;
    }
}

/// Whether a byte slice contains any §12.3 LineTerminator — LF, CR, or
/// the UTF-8 spellings of U+2028 / U+2029.
fn has_line_break(bytes: &[u8]) -> bool {
    (0..bytes.len()).any(|k| util::line_terminator_at(bytes, k).is_some())
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
