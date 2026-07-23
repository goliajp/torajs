//! Computed-property arm of the object-literal parser — split out as
//! the rotation-197 file-size sweep. The parent `object_literal.rs`
//! drifted to 507 prod LOC after the `{ async [computedKey]() {} }`
//! stub-drop + `{ [expr]() {} }` method-shorthand wedges landed on
//! top of the original literal-string / Symbol.X-chain key path.
//!
//! Covers V3-18 P2.4.c.4 + P0.10:
//! - `{ [<StringLit>]: v }` rewrites to `{ <StringLit>: v }` at parse
//!   time (struct layouts are static; runtime keys await a dictionary
//!   substrate).
//! - `{ [Symbol.iterator]: v }` / `{ [Foo.bar]: v }` — Member chain keys
//!   parsed and encoded as the synthetic name `__sym_<chain>__`; the
//!   real iterator-protocol dispatch lands with Phase E.
//! - `{ [expr]() {} }` — computed-key method shorthand (ES §13.2.5
//!   ComputedPropertyName + MethodDefinition). Body brace-balanced +
//!   emits a null stub; the real Symbol.X dispatch lands with P3/P7.
//!
//! Verbatim move; token / grammar behavior unchanged.

use super::*;

impl<'a> Parser<'a> {
    /// `{ [<key>]: v }` / `{ [<key>](){} }` computed-property arm.
    /// Returns `Ok(Some(pair))` when the leading `[` has been consumed
    /// and the whole property has been parsed; `Ok(None)` when the
    /// current token is not `[` (caller falls through to the
    /// name-based property paths); `Err` on a mid-computed-key parse
    /// error.
    pub(super) fn try_parse_computed_property(
        &mut self,
    ) -> Result<Option<(String, ExprId)>, String> {
        if !matches!(self.peek(), Token::LBracket) {
            return Ok(None);
        }
        self.pos += 1;
        let key = match self.peek() {
            Token::String(s) => {
                let key = s.clone();
                self.pos += 1;
                key
            }
            Token::Ident(_) => {
                // Try Member chain like `Symbol.iterator` / `Foo.bar`.
                // Encode as `__sym_<chain>__` for the field name.
                let mut parts: Vec<String> = Vec::new();
                loop {
                    if let Token::Ident(n) = self.peek() {
                        parts.push(n.clone());
                        self.pos += 1;
                    } else {
                        break;
                    }
                    if matches!(self.peek(), Token::Dot) {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                format!("__sym_{}__", parts.join("_"))
            }
            t => {
                return Err(format!(
                    "subset: computed property key must be a literal string, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        match self.peek() {
            Token::RBracket => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `]` after computed property key, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // P0.10 — computed-key method shorthand `{ [expr]() { ... } }`
        // per ES spec §13.2.5 ComputedPropertyName + MethodDefinition.
        // Used pervasively for `Symbol.toPrimitive` / `Symbol.iterator`
        // hooks. tora has no Symbol.X dispatch substrate (lands with
        // P3 / P7 iterator-protocol), so the field carries a stub
        // value just like getter/setter shorthand. The parse must
        // succeed so the surrounding object literal still compiles.
        if matches!(self.peek(), Token::LParen) {
            // Drop the param list with paren-balance.
            let mut depth = 1i32;
            self.pos += 1;
            while depth > 0 {
                match self.peek() {
                    Token::LParen => depth += 1,
                    Token::RParen => depth -= 1,
                    Token::Eof => {
                        return Err(format!(
                            "unexpected eof in computed-key method shorthand params at {}",
                            self.at()
                        ));
                    }
                    _ => {}
                }
                self.pos += 1;
            }
            // Optional return type ann.
            if matches!(self.peek(), Token::Colon) {
                self.pos += 1;
                let _ = self.parse_type_ann()?;
            }
            // Drop the body with brace-balance.
            match self.peek() {
                Token::LBrace => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `{{` after computed-key method shorthand header, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            let mut depth = 1i32;
            while depth > 0 {
                match self.peek() {
                    Token::LBrace => depth += 1,
                    Token::RBrace => depth -= 1,
                    Token::Eof => {
                        return Err(format!(
                            "unexpected eof in computed-key method shorthand body at {}",
                            self.at()
                        ));
                    }
                    _ => {}
                }
                self.pos += 1;
            }
            let value = self.ast.add_expr(Expr::Null);
            return Ok(Some((key, value)));
        }
        match self.peek() {
            Token::Colon => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `:` after `[<key>]` in object literal, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let value = self.parse_assign()?;
        Ok(Some((key, value)))
    }
}
