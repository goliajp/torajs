//! `RegExp.escape` — ES2025 §22.2.5.1 (rotation 266).
//!
//! EncodeForRegExpEscape over the code points of a Str payload:
//! - a leading ASCII letter / decimal digit hex-escapes (`\x68ello`)
//!   so the result can never fuse with a preceding pattern token;
//! - SyntaxCharacter (`^ $ \ . * + ? ( ) [ ] { } |`) and `/` take a
//!   backslash prefix;
//! - the ControlEscape table (TAB LF VT FF CR) answers `\t \n \v \f
//!   \r`;
//! - otherPunctuators (`, - = < > # & ! % : ; @ ~ ' \` "`), the
//!   remaining WhiteSpace (SP NBSP ZWNBSP + Zs) and the
//!   LineTerminators LS / PS hex-escape (`\xhh` under U+0100,
//!   `\uhhhh` above);
//! - everything else passes through verbatim.
//!
//! The §22.2.5.1 step-1 "not a String → TypeError" gate lives in the
//! caller (`torajs-anyvalue`'s any shell / the typed lowering) — this
//! kernel takes a live Str and always answers a fresh Str. Lone
//! surrogates cannot occur in tr's well-formed UTF-8 payloads, so
//! that EncodeForRegExpEscape arm has no reachable input here.

use core::ffi::c_void;

use alloc::string::String;
use alloc::vec::Vec;

use super::str_helpers::{str_code_points, str_from_bytes};

/// SyntaxCharacter (§22.2.1) plus U+002F `/`.
fn is_syntax_or_slash(c: char) -> bool {
    matches!(
        c,
        '^' | '$' | '\\' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '/'
    )
}

/// The table-49 otherPunctuators set.
fn is_other_punctuator(c: char) -> bool {
    matches!(
        c,
        ',' | '-'
            | '='
            | '<'
            | '>'
            | '#'
            | '&'
            | '!'
            | '%'
            | ':'
            | ';'
            | '@'
            | '~'
            | '\''
            | '`'
            | '"'
    )
}

/// WhiteSpace / LineTerminator members NOT already covered by the
/// ControlEscape table: SP, NBSP, ZWNBSP, the Zs block, LS, PS.
fn is_ws_or_lt(c: char) -> bool {
    matches!(
        c,
        ' ' | '\u{a0}' | '\u{feff}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{2028}' | '\u{2029}'
    )
}

fn push_hex_escape(out: &mut String, c: char) {
    let cp = c as u32;
    if cp < 0x100 {
        out.push_str("\\x");
        push_hex(out, cp, 2);
    } else {
        // Every member of the hex-escape classes is BMP, so one
        // UTF-16 code unit == the code point.
        out.push_str("\\u");
        push_hex(out, cp, 4);
    }
}

fn push_hex(out: &mut String, v: u32, width: u32) {
    for i in (0..width).rev() {
        let d = (v >> (i * 4)) & 0xF;
        out.push(char::from_digit(d, 16).unwrap());
    }
}

/// §22.2.5.1 steps 2-6 over a UTF-8 payload. Answers a fresh Str.
///
/// # Safety
/// `s` is a live tora Str pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regexp_escape(s: *const c_void) -> *mut u8 {
    let bytes: Vec<u8> = unsafe { str_code_points(s) };
    let text = core::str::from_utf8(&bytes).unwrap_or("");
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if out.is_empty() && c.is_ascii_alphanumeric() {
            out.push_str("\\x");
            push_hex(&mut out, c as u32, 2);
        } else if is_syntax_or_slash(c) {
            out.push('\\');
            out.push(c);
        } else if let Some(letter) = match c {
            '\t' => Some('t'),
            '\n' => Some('n'),
            '\u{b}' => Some('v'),
            '\u{c}' => Some('f'),
            '\r' => Some('r'),
            _ => None,
        } {
            out.push('\\');
            out.push(letter);
        } else if is_other_punctuator(c) || is_ws_or_lt(c) {
            push_hex_escape(&mut out, c);
        } else {
            out.push(c);
        }
    }
    unsafe { str_from_bytes(out.as_bytes()) }
}
