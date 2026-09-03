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

/// §12.3 LineTerminator — LF, CR, and the UTF-8 spellings of U+2028
/// LINE SEPARATOR / U+2029 PARAGRAPH SEPARATOR. Returns the width in
/// bytes of the one starting at `i` (a CR LF pair counts as one, two
/// bytes wide), `None` when no line terminator starts there.
///
/// The set is closed and small, which is exactly why it kept being
/// respelled: the line-comment scanner, the HTML-comment scanner, the
/// no-line-break-here checks and the regex-literal body each wrote
/// their own two-branch version, and the regex one had only ever
/// grown the LF branch. One spelling, one place to read it. (The
/// `\ LineTerminatorSequence` continuation inside a string or
/// template is the same character set but a different production —
/// it consumes rather than stops — and stays in its own scanners.)
pub(super) fn line_terminator_at(bytes: &[u8], i: usize) -> Option<usize> {
    let crlf = bytes.get(i + 1) == Some(&b'\n');
    match *bytes.get(i)? {
        b'\n' => Some(1),
        b'\r' if crlf => Some(2),
        b'\r' => Some(1),
        0xE2 if bytes.get(i + 1) == Some(&0x80)
            && matches!(bytes.get(i + 2), Some(&0xA8) | Some(&0xA9)) =>
        {
            Some(3)
        }
        _ => None,
    }
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

/// Offset of the first backslash in `bytes[from..to]` whose escape
/// `pred` rejects, or `None`.
///
/// Steps two bytes per backslash rather than decoding, which is safe
/// because no longer escape (`\xNN`, `\uNNNN`, `\u{…}`) contains a
/// backslash for the walk to land inside of, and an escaped backslash
/// (`\\101`) is stepped over as the pair it is. Callers pass a whole
/// literal's span, quotes and all — a quote is never a backslash.
///
/// One walk, two questions: [`super::legacy_octal::first_legacy_escape`]
/// asks which escapes a strict goal refuses, [`first_malformed_hex_escape`]
/// asks which ones are not escapes at all. A second copy of the step rule
/// would not error — it would start lying the day one copy learned
/// something the other had not.
pub(super) fn first_escape_where(
    bytes: &[u8],
    from: u32,
    to: u32,
    pred: impl Fn(&[u8], u32) -> bool,
) -> Option<u32> {
    let mut i = from;
    while i + 1 < to {
        if bytes[i as usize] == b'\\' {
            if pred(bytes, i) {
                return Some(i);
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    None
}

/// Offset of the first `\x` / `\u` in `bytes[from..to]` that no
/// UnicodeEscapeSequence / HexEscapeSequence production accepts, or
/// `None`.
///
/// §12.9.4 EscapeCharacter lists `x` and `u`, so neither can fall back
/// to NonEscapeCharacter: a malformed spelling is not a literal `x` /
/// `u`, it is nothing at all. The string scanner raises it on the spot;
/// an UNTAGGED template asks here, over the raw text, because a tagged
/// one is allowed the same spelling (§12.9.6 gives it `undefined` as
/// the cooked value instead of an error).
///
/// [`scan_hex_escape`] is the only judge of well-formedness — this
/// walks, it does not re-count hex digits.
pub(crate) fn first_malformed_hex_escape(bytes: &[u8], from: u32, to: u32) -> Option<u32> {
    first_escape_where(bytes, from, to, |b, i| {
        matches!(b.get((i + 1) as usize), Some(b'x' | b'u')) && scan_hex_escape(b, i).is_none()
    })
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
