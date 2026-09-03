//! Free helpers shared across `parser/{atom,escape,class,class_v}` —
//! extracted from [`super`] as the rotation-196 file-size sweep
//! (the parent had drifted to 506 prod LOC over the recent
//! strictness wave). Verbatim moves; nothing about signatures /
//! semantics changes. `parser/mod.rs` re-exports every symbol so
//! sibling callers keep `use super::{char_node, ...};` byte-for-byte
//! unchanged.

use alloc::boxed::Box;

use crate::charclass::CharClass;
use crate::node::{Node, NodeKind};

/// §22.2.1 CharacterEscape :: `c` ControlLetter — ControlLetter is
/// `[A-Za-z]`, and the escape's value is the letter's code modulo 32.
///
/// AnnexB §B.1.4 widens the set to ClassControlLetter :: DecimalDigit
/// | `_`, but only INSIDE A CLASS and only outside u/v. That widening
/// is the entire difference between the three sites that ask this —
/// the atom parser, the class parser, and the v-mode class parser —
/// so it is a parameter here rather than a third spelling of the rule.
pub(crate) fn control_letter_value(b: u8, class_annex_b: bool) -> Option<u32> {
    (b.is_ascii_alphabetic() || (class_annex_b && (b.is_ascii_digit() || b == b'_')))
        .then(|| u32::from(b) % 32)
}

/// ES §22.2.1.1 SyntaxCharacter — the punctuation reserved by the
/// pattern grammar, and so the valid targets for IdentityEscape under
/// u/v. `/` joins them (§22.2.1.1 admits it for the literal form).
/// Asked by the atom parser and, for ClassEscape, by the class parser.
pub(crate) fn is_regex_syntax_char(b: u8) -> bool {
    matches!(
        b,
        b'^' | b'$'
            | b'\\'
            | b'.'
            | b'*'
            | b'+'
            | b'?'
            | b'('
            | b')'
            | b'['
            | b']'
            | b'{'
            | b'}'
            | b'|'
            | b'/'
    )
}

pub(crate) fn char_node(ch: u8) -> Box<Node> {
    let mut n = Node::new(NodeKind::Char);
    n.ch = ch;
    n
}

pub(crate) fn class_node<F: FnOnce(&mut CharClass)>(populate: F) -> Box<Node> {
    let mut n = Node::new(NodeKind::Class);
    populate(&mut n.cc);
    n
}

pub(crate) fn hex_value(h: u8) -> Option<u8> {
    if h.is_ascii_digit() {
        Some(h - b'0')
    } else if (b'a'..=b'f').contains(&h) {
        Some(h - b'a' + 10)
    } else if (b'A'..=b'F').contains(&h) {
        Some(h - b'A' + 10)
    } else {
        None
    }
}

/// Word-byte predicate used by `\p{}` property-name parsing. Matches
/// `[A-Za-z0-9_]` (ASCII only — same as C port). Public so the future
/// VM (P6.2-e) can reuse for `\b` word-boundary. A capture-group name
/// is an identifier, not a word-byte run — it reads through
/// [`super::Parser::read_group_name`].
pub fn is_word_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Apply a Unicode property expression (`\p{Name}` /
/// `\p{Name=Value}`) to the char-class on `n` per ES §22.2.1
/// UnicodePropertyValueExpression. Returns `false` on unknown
/// name/value (→ parser SyntaxError, spec early-error semantics).
///
/// Keyed form: only `General_Category` / `gc`, `Script` / `sc`,
/// `Script_Extensions` / `scx` are valid keys, each resolving the
/// value in its own table family. Lone form: binary property names
/// first (`Alphabetic`, `White_Space`, …), then bare
/// `General_Category` values (`Lu`, `Letter`, …).
pub(crate) fn apply_property_name(n: &mut Node, name: &[u8], value: Option<&[u8]>) -> bool {
    use crate::ucd::{lookup_binary, lookup_gc_value, lookup_script, lookup_scx};
    let Ok(name) = core::str::from_utf8(name) else {
        return false;
    };
    let table = match value {
        Some(v) => {
            let Ok(v) = core::str::from_utf8(v) else {
                return false;
            };
            match name {
                "General_Category" | "gc" => lookup_gc_value(v),
                "Script" | "sc" => lookup_script(v),
                "Script_Extensions" | "scx" => lookup_scx(v),
                _ => None,
            }
        }
        None => lookup_binary(name).or_else(|| lookup_gc_value(name)),
    };
    match table {
        Some(t) => {
            n.cc.add_property_table(t);
            true
        }
        None => false,
    }
}
