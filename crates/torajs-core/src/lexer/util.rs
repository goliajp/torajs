//! Lexer scanning primitives — `advance` / `peek` / `regex_context`
//! / `emit` / `push_codepoint` / `is_ident_start` / `is_ident_cont`.
//!
//! All `pub(super)` so `tokenize` (in `lexer.rs`) can call them
//! without leaking them through the crate's public API.
//!
//! Extracted from `lexer.rs` (2026-05-25, god-file decomp batch 20).

use super::{Span, Spanned, Token};

pub(super) fn advance(i: &mut u32) -> u32 {
    *i += 1;
    *i
}

pub(super) fn peek(bytes: &[u8], i: u32) -> Option<u8> {
    bytes.get(i as usize).copied()
}

/// JS lexer ambiguity: `/` is a regex-literal start when the previous
/// token is a punctuator that can begin an expression on its right
/// or a keyword like `return` / `typeof` / etc.; otherwise it's a
/// division operator. Mirrors what V8 / SpiderMonkey / JSC do.
pub(super) fn regex_context(prev: Option<&Token>) -> bool {
    let Some(t) = prev else {
        // Start of file — anything goes; default-yes.
        return true;
    };
    matches!(
        t,
        // Punctuators
        Token::LParen
            | Token::LBrace
            | Token::LBracket
            | Token::Comma
            | Token::Semi
            | Token::Colon
            | Token::Question
            | Token::QuestionQuestion
            | Token::QuestionDot
            | Token::Bang
            | Token::Tilde
            | Token::Plus
            | Token::Minus
            | Token::Star
            | Token::Slash
            | Token::Percent
            | Token::Eq
            | Token::EqEqEq
            | Token::BangEqEq
            | Token::EqEq
            | Token::BangEq
            | Token::Lt
            | Token::Gt
            | Token::LtEq
            | Token::GtEq
            | Token::Amp
            | Token::AmpAmp
            | Token::Pipe
            | Token::PipePipe
            | Token::Caret
            | Token::ShlShl
            | Token::ShrShr
            | Token::ShrShrShr
            | Token::FatArrow
            | Token::DotDotDot
            | Token::SlashEq
            | Token::PlusEq
            | Token::MinusEq
            | Token::StarEq
            | Token::PercentEq
            // Expression-starting keywords
            | Token::Return
            | Token::TypeOf
            | Token::Void
            | Token::InstanceOf
            | Token::New
            | Token::Throw
            | Token::Case
            | Token::Yield
            | Token::Await
            | Token::Else
            | Token::Do
            | Token::If
            | Token::While
            | Token::For
    )
}

pub(super) fn emit(out: &mut Vec<Spanned>, token: Token, start: u32, end: u32) {
    out.push(Spanned {
        token,
        span: Span { start, end },
    });
}

/// Encode a Unicode code point as UTF-8 into `buf`. Used by string-
/// literal escape decoding (`\xNN`, `\uNNNN`, `\u{N...N}`). Codepoints
/// past U+10FFFF or in the surrogate range fall back to U+FFFD
/// REPLACEMENT CHARACTER — matches V8's recovery behavior on malformed
/// escapes.
pub(super) fn push_codepoint(buf: &mut Vec<u8>, cp: u32) {
    let c = char::from_u32(cp).unwrap_or('\u{FFFD}');
    let mut tmp = [0u8; 4];
    let s = c.encode_utf8(&mut tmp);
    buf.extend_from_slice(s.as_bytes());
}

pub(super) fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}

pub(super) fn is_ident_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}
