//! Reserved-word property-name table — sibling of `parser.rs`
//! (rotation 204 file-size split: the `Token::Var` arm pushed the
//! known-debt host past its recorded baseline; table verbatim).

use super::*;

impl<'a> Parser<'a> {
    /// V3-18 wedge — return the source spelling of `t` if it's a
    /// reserved-word keyword that can appear in property-name
    /// contexts (object-literal field, member access, destructuring
    /// pattern, class member). Per ES spec §12.7.6 IdentifierName
    /// allows the full reserved-word list at these positions; TS
    /// follows. Used by member_name_after_dot, parse_object_field,
    /// parse_object_destructuring, and the class-member-name branch
    /// in parse_class — all four had their own short keyword whitelist
    /// that drifted apart over time. Centralized here.
    pub(super) fn keyword_property_name(t: &Token) -> Option<&'static str> {
        Some(match t {
            Token::Catch => "catch",
            Token::Finally => "finally",
            Token::Return => "return",
            Token::Throw => "throw",
            Token::If => "if",
            Token::Else => "else",
            Token::For => "for",
            Token::While => "while",
            Token::Do => "do",
            Token::Break => "break",
            Token::Continue => "continue",
            Token::Switch => "switch",
            Token::Case => "case",
            Token::Default => "default",
            Token::Class => "class",
            Token::New => "new",
            Token::This => "this",
            Token::Function => "function",
            Token::TypeOf => "typeof",
            Token::Delete => "delete",
            Token::InstanceOf => "instanceof",
            Token::Try => "try",
            Token::Yield => "yield",
            // Extended set — these were rejected pre-wedge in
            // every property-name position.
            Token::Type => "type",
            Token::Async => "async",
            Token::Await => "await",
            Token::Import => "import",
            Token::Export => "export",
            Token::Null => "null",
            Token::True => "true",
            Token::False => "false",
            Token::Let => "let",
            Token::Const => "const",
            Token::Extends => "extends",
            Token::Super => "super",
            Token::Void => "void",
            // Rotation 204 — `var` was the one keyword with its own
            // token missing from this table (`in` / `with` / `enum`
            // etc. lex as Ident and ride the Ident arm): `obj.var`
            // was a parse error (test262
            // ident-name-keyword-memberexpr).
            Token::Var => "var",
            _ => return None,
        })
    }
}
