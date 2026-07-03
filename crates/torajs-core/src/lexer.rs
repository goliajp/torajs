//! Lexer — TS-shaped token stream. Subset for P0.2 (just enough for
//! `console.log("hello")`). The big match-arm tokenize loop lives in
//! this file; public types in `lexer/types.rs`, scanning primitives
//! (`advance` / `peek` / `regex_context` / `emit` / ...) in
//! `lexer/util.rs`.

mod scan;
mod scan_number;
mod scan_slash;
mod scan_template;
mod types;
mod util;

pub use types::{Span, Spanned, TemplatePart, Token};

use util::{advance, emit, is_ident_start, peek};

pub fn tokenize(src: &str) -> Result<Vec<Spanned>, String> {
    let bytes = src.as_bytes();
    let len = bytes.len() as u32;
    let mut out = Vec::new();
    let mut i: u32 = 0;

    while i < len {
        let start = i;
        let b = bytes[i as usize];
        match b {
            b' ' | b'\t' | b'\r' | b'\n' => {
                i += 1;
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
            b'>' => {
                i += 1;
                if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    emit(&mut out, Token::GtEq, start, i);
                } else if peek(bytes, i) == Some(b'>') {
                    i += 1;
                    if peek(bytes, i) == Some(b'>') {
                        i += 1;
                        emit(&mut out, Token::ShrShrShr, start, i);
                    } else {
                        emit(&mut out, Token::ShrShr, start, i);
                    }
                } else {
                    emit(&mut out, Token::Gt, start, i);
                }
            }
            b'=' => {
                i += 1;
                if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    if peek(bytes, i) == Some(b'=') {
                        i += 1;
                        emit(&mut out, Token::EqEqEq, start, i);
                    } else {
                        // V3-18 m3 — `==` IsLooselyEqual per §7.2.13.
                        // Restored from "out-of-scope" 2026-05-10
                        // (test262 100% bar). Emits a new
                        // Token::EqEq → BinOp::LooseEq.
                        emit(&mut out, Token::EqEq, start, i);
                    }
                } else if peek(bytes, i) == Some(b'>') {
                    i += 1;
                    emit(&mut out, Token::FatArrow, start, i);
                } else {
                    emit(&mut out, Token::Eq, start, i);
                }
            }
            b'!' => {
                i += 1;
                if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    if peek(bytes, i) == Some(b'=') {
                        i += 1;
                        emit(&mut out, Token::BangEqEq, start, i);
                    } else {
                        // V3-18 m3 — `!=` is `!IsLooselyEqual`.
                        emit(&mut out, Token::BangEq, start, i);
                    }
                } else {
                    // Unary logical not — used as `!cond`. M1.5.
                    emit(&mut out, Token::Bang, start, i);
                }
            }
            b'"' | b'\'' => scan::scan_string(bytes, &mut i, &mut out, start, len)?,
            b'`' => scan_template::scan_template(bytes, &mut i, &mut out, start, len)?,
            b'#' if peek(bytes, i + 1).is_some_and(is_ident_start) => {
                // P8.1 — `#name` PrivateIdentifier; a bare `#` not
                // followed by an ident start falls through to the
                // unexpected-byte error below.
                scan::scan_private_ident(bytes, &mut i, &mut out, start, len)
            }
            b if is_ident_start(b) => {
                scan::scan_ident_or_keyword(bytes, &mut i, &mut out, start, len)
            }
            b if b.is_ascii_digit() => {
                scan_number::scan_number(bytes, &mut i, &mut out, start, len, b)?
            }
            _ => return Err(format!("unexpected byte {b:#x} at {start}")),
        }
    }
    emit(&mut out, Token::Eof, len, len);
    Ok(out)
}
