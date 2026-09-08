//! Literal type annotations — `"dev" | "prod"`, `0 | 1`, `true`.
//!
//! TS §3.2.10 makes a literal a type whose only value is that
//! literal. The subset does not carry singleton types, so each of the
//! three widens to its base type and swallows any `|`-chain of further
//! literals of the same kind, which all collapse to that same base.
//!
//! Split out of `type_ann.rs` (2026-09-08): `parse_type_ann_inner` is
//! a registered size-debt entry, so growing it in place to admit the
//! `null` type annotation was not available. Nothing here changed —
//! the three arms answer exactly what they answered inline, and the
//! `Option` return says "not a literal, keep going" where the inline
//! version simply fell through.

use super::*;

impl Parser<'_> {
    /// The base type a literal type annotation widens to, or `None`
    /// when the next token does not start one.
    pub(super) fn literal_type_widen(&mut self) -> Option<String> {
        // V3-18 wedge — string-literal type-ann (`type Mode =
        // "dev" | "prod"`). Per TS spec §3.2.10 a string literal
        // is a type that has only that literal as its value. The
        // subset widens to plain `string` and consumes any
        // following `| "..."` chain (treating them as further
        // string-literal alternatives that all collapse to the
        // same `string`).
        if let Token::String(_) = self.peek() {
            self.pos += 1;
            while matches!(self.peek(), Token::Pipe)
                && matches!(
                    self.tokens.get(self.pos + 1).map(|s| &s.token),
                    Some(Token::String(_))
                )
            {
                self.pos += 2;
            }
            return Some("string".to_string());
        }
        // V3-18 wedge — number-literal type-ann (`type Bit =
        // 0 | 1`). Same widening to plain `number`.
        if let Token::Number(_) = self.peek() {
            self.pos += 1;
            while matches!(self.peek(), Token::Pipe)
                && matches!(
                    self.tokens.get(self.pos + 1).map(|s| &s.token),
                    Some(Token::Number(_))
                )
            {
                self.pos += 2;
            }
            return Some("number".to_string());
        }
        // V3-18 wedge — boolean-literal type-ann (`type Always =
        // true`). Same widening to plain `boolean`.
        if matches!(self.peek(), Token::True | Token::False) {
            self.pos += 1;
            while matches!(self.peek(), Token::Pipe)
                && matches!(
                    self.tokens.get(self.pos + 1).map(|s| &s.token),
                    Some(Token::True) | Some(Token::False)
                )
            {
                self.pos += 2;
            }
            return Some("boolean".to_string());
        }
        None
    }
}
