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
            | Token::Delete
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

/// Encode a code point as WTF-8 into `buf`. Used by string-literal
/// escape decoding (`\xNN`, `\uNNNN`, `\u{N...N}`). A lone surrogate
/// keeps its own 3-byte spelling — the string value is a sequence of
/// UTF-16 code units (§6.1.4), not of scalar values — and a high
/// surrogate escape followed by a low one joins into one code point
/// (§12.9.4.2 SV). Code points past U+10FFFF cannot reach here: the
/// `\u{…}` scanner rejects them.
pub(super) fn push_codepoint(buf: &mut Vec<u8>, cp: u32) {
    torajs_wtf8::push_code_point(buf, cp);
}

/// A hex-valued escape at `bytes[i] == b'\\'` — `\xNN`, `\uNNNN` or
/// `\u{N…N}` (§12.9.4.1 HexEscapeSequence / UnicodeEscapeSequence;
/// the braced form takes any digit count whose value stays within
/// U+10FFFF). Returns the code point and the escape's byte length;
/// `None` when the digits are not there, and the caller decides
/// between passthrough and an error. Shared by the string-literal and
/// template scanners so both cook the same set.
pub(super) fn scan_hex_escape(bytes: &[u8], i: u32) -> Option<(u32, u32)> {
    let at = |k: u32| bytes.get((i + k) as usize).copied();
    let hex = |b: u8| (b as char).to_digit(16);
    match at(1)? {
        b'x' => {
            let hi = hex(at(2)?)?;
            let lo = hex(at(3)?)?;
            Some((hi * 16 + lo, 4))
        }
        b'u' if at(2) == Some(b'{') => {
            let mut k = 3;
            let mut cp: u32 = 0;
            while let Some(d) = at(k).and_then(hex) {
                cp = cp.saturating_mul(16).saturating_add(d);
                k += 1;
            }
            (k > 3 && at(k) == Some(b'}') && cp <= 0x10FFFF).then_some((cp, k + 1))
        }
        b'u' => {
            let mut cp: u32 = 0;
            for k in 2..=5 {
                cp = cp * 16 + hex(at(k)?)?;
            }
            Some((cp, 6))
        }
        _ => None,
    }
}

pub(super) fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}

pub(super) fn is_ident_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// Decode the UTF-8 sequence at `i`, returning `(codepoint, byte width)`.
/// `None` when the bytes there are not a valid sequence — the caller then
/// leaves `i` alone and reports the raw byte, so malformed input keeps
/// producing the same lex error it always did.
///
/// The lexer only reaches this on a byte ≥ 0x80: every ASCII path is a
/// match arm above it, so identifiers stay a pure byte walk in the common
/// case and pay decoding only where a non-ASCII codepoint really appears.
pub(super) fn decode_utf8(bytes: &[u8], i: u32) -> Option<(u32, u32)> {
    let width = match *bytes.get(i as usize)? {
        0x00..=0x7F => 1,
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => return None,
    };
    // Doubles as the continuation-byte check: a truncated or malformed
    // sequence fails here and the caller falls through to its error path.
    let c = core::str::from_utf8(bytes.get(i as usize..i as usize + width)?)
        .ok()?
        .chars()
        .next()?;
    Some((c as u32, width as u32))
}
