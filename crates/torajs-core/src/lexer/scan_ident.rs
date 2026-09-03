//! Identifier scanning — ES2024 §12.7.1.
//!
//! ```text
//! IdentifierStart :: IdentifierStartChar | \ UnicodeEscapeSequence
//! IdentifierPart  :: IdentifierPartChar  | \ UnicodeEscapeSequence
//! ```
//!
//! A name is its *code points*, not its spelling: `\u{6F}` and `o` name
//! the same binding, and the escape is checked against the very same
//! `ID_Start` / `ID_Continue` tables a literal character would be. What
//! an escape cannot do is manufacture a keyword — §12.7.2 makes an
//! escaped `ReservedWord` a Syntax Error rather than either a keyword or
//! an identifier, so the keyword table is consulted only for names that
//! were spelled literally.
//!
//! Split out of `scan.rs` when escapes arrived (the two scanners plus the
//! escape decoder would have pushed that file past its size budget).

use super::util::{decode_utf8, emit, is_ident_cont, is_ident_start, push_codepoint};
use super::{Spanned, Token};

/// ES §12.7.2 `ReservedWord`. This table is consulted ONLY for names
/// an escape contributed to (`scan_ident_or_keyword`'s escaped
/// branch) — literal spellings go through `keyword_or_ident` and
/// never read it. `await` and `yield` are conditionally reserved
/// (modules/async bodies resp. strict/generator bodies), but tr's
/// corpus is module-goal strict where both are reserved, so their
/// ESCAPED spellings reject here like any keyword (bun parity:
/// "The keyword X cannot be escaped"); their LITERAL spellings keep
/// the parser's context-sensitive handling untouched.
const RESERVED_WORDS: &[&str] = &[
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "new",
    "null",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

/// ES §12.7.1 `IdentifierStartChar`, as a code point rather than a byte —
/// the form the escape path needs, since `\u{6F}` resolves to ASCII `o`.
fn cp_starts_ident(cp: u32) -> bool {
    if cp < 0x80 {
        return is_ident_start(cp as u8);
    }
    torajs_ucd::is_id_start_cp(cp)
}

/// ES §12.7.1 `IdentifierPartChar`, as a code point. See [`cp_starts_ident`].
/// `scan_slash` reads it too — regex-literal flags are an
/// IdentifierPart run (§12.9.5), not a run of letters.
pub(super) fn cp_continues_ident(cp: u32) -> bool {
    if cp < 0x80 {
        return is_ident_cont(cp as u8);
    }
    torajs_ucd::is_id_continue_cp(cp)
}

fn hex4(bytes: &[u8], at: usize) -> Option<u32> {
    let mut cp = 0u32;
    for k in 0..4 {
        cp = cp * 16 + (*bytes.get(at + k)? as char).to_digit(16)?;
    }
    Some(cp)
}

/// Decode the `\ UnicodeEscapeSequence` whose backslash sits at `i`,
/// returning `(code point, byte width including the backslash)`.
///
/// Both spellings from §12.9.4.1 are accepted. A high-surrogate escape
/// immediately followed by a low-surrogate one is a single supplementary
/// code point, the same UTF-16 string-value rule the string scanner
/// applies — without it no supplementary identifier could be spelled in
/// the `\uXXXX` form at all.
pub(super) fn decode_ident_escape(bytes: &[u8], i: u32) -> Option<(u32, u32)> {
    let at = i as usize;
    if *bytes.get(at)? != b'\\' || *bytes.get(at + 1)? != b'u' {
        return None;
    }
    if *bytes.get(at + 2)? == b'{' {
        let mut k = at + 3;
        let mut cp = 0u32;
        let mut digits = 0;
        while let Some(d) = bytes.get(k).and_then(|b| (*b as char).to_digit(16)) {
            cp = cp.checked_mul(16)?.checked_add(d)?;
            k += 1;
            digits += 1;
        }
        if digits == 0 || *bytes.get(k)? != b'}' || cp > 0x10FFFF {
            return None;
        }
        return Some((cp, (k + 1 - at) as u32));
    }

    let cp = hex4(bytes, at + 2)?;
    if (0xD800..=0xDBFF).contains(&cp)
        && bytes.get(at + 6) == Some(&b'\\')
        && bytes.get(at + 7) == Some(&b'u')
        && let Some(lo) = hex4(bytes, at + 8)
        && (0xDC00..=0xDFFF).contains(&lo)
    {
        return Some((0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00), 12));
    }
    Some((cp, 6))
}

/// Whether an `IdentifierStart` begins at `i` — a literal start character
/// or an escape that resolves to one. `tokenize`'s identifier arm and its
/// `#` arm both ask this, so `#\u{6F}` and `#o` admit the same name.
pub(super) fn ident_start_at(bytes: &[u8], i: u32) -> bool {
    match bytes.get(i as usize) {
        Some(b'\\') => decode_ident_escape(bytes, i).is_some_and(|(cp, _)| cp_starts_ident(cp)),
        Some(&b) if b < 0x80 => is_ident_start(b),
        Some(_) => decode_utf8(bytes, i).is_some_and(|(cp, _)| cp_starts_ident(cp)),
        None => false,
    }
}

/// Consume one `IdentifierName` starting at `i`, returning its decoded
/// text and whether any escape contributed to it. Stops without consuming
/// at the first character that cannot continue the name, so the caller's
/// span always ends on a code-point boundary.
fn scan_ident_name(bytes: &[u8], i: &mut u32, len: u32) -> Result<(String, bool), String> {
    let mut buf: Vec<u8> = Vec::new();
    let mut escaped = false;
    let mut first = true;

    while *i < len {
        let b = bytes[*i as usize];
        let admits: fn(u32) -> bool = if first {
            cp_starts_ident
        } else {
            cp_continues_ident
        };

        if b == b'\\' {
            let (cp, w) = decode_ident_escape(bytes, *i)
                .ok_or_else(|| format!("malformed unicode escape in identifier at {i}"))?;
            if !admits(cp) {
                return Err(format!(
                    "\\u{cp:04X} is not a valid identifier {} at {i}",
                    if first { "start" } else { "part" }
                ));
            }
            push_codepoint(&mut buf, cp);
            *i += w;
            escaped = true;
        } else if b < 0x80 {
            if !admits(b as u32) {
                break;
            }
            buf.push(b);
            *i += 1;
        } else {
            match decode_utf8(bytes, *i) {
                Some((cp, w)) if admits(cp) => {
                    push_codepoint(&mut buf, cp);
                    *i += w;
                }
                _ => break,
            }
        }
        first = false;
    }

    let name = String::from_utf8(buf).expect("identifier buffer is built from code points");
    Ok((name, escaped))
}

pub(super) fn scan_ident_or_keyword(
    bytes: &[u8],
    i: &mut u32,
    out: &mut Vec<Spanned>,
    start: u32,
    len: u32,
) -> Result<(), String> {
    let (name, escaped) = scan_ident_name(bytes, i, len)?;
    if escaped {
        // §12.7.2 — an escaped ReservedWord is not the keyword, but it
        // IS a valid IdentifierName, so `({ break: 1 })` and
        // `o.break` are legal while `if (x) {}` is not. The
        // lexer cannot tell those apart — position is the parser's
        // business — so hand over a token the property-name positions
        // opt into and everything else refuses by construction. (The
        // blanket refusal that used to live here is what kept
        // `if (x) {}` from quietly becoming a call to a function named
        // `if`; `Token::EscapedIdent` keeps that guarantee without
        // taking the legal positions down with it.)
        if RESERVED_WORDS.contains(&name.as_str()) {
            emit(out, Token::EscapedIdent(name), start, *i);
            return Ok(());
        }
        emit(out, Token::Ident(name), start, *i);
        return Ok(());
    }
    emit(out, keyword_or_ident(name), start, *i);
    Ok(())
}

/// `#name` PrivateIdentifier (P8.1). The name after `#` obeys the same
/// production as any other identifier, escapes included; the keyword rule
/// does not apply, since `#if` is a perfectly ordinary private name.
pub(super) fn scan_private_ident(
    bytes: &[u8],
    i: &mut u32,
    out: &mut Vec<Spanned>,
    start: u32,
    len: u32,
) -> Result<(), String> {
    *i += 1;
    let (name, _) = scan_ident_name(bytes, i, len)?;
    emit(out, Token::PrivateIdent(name), start, *i);
    Ok(())
}

fn keyword_or_ident(name: String) -> Token {
    match name.as_str() {
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
        _ => Token::Ident(name),
    }
}
