//! Object type literals — `{ x: T; y: U }`, encoded as
//! `__inlobj(x:T|y:U)` for downstream check.rs to decode into a
//! `Type::Struct` (same encoding scheme as `__fn(...)`).

use super::*;

impl Parser<'_> {
    /// The object type literal at the cursor (`{` not yet consumed).
    pub(super) fn parse_inline_obj_type(&mut self) -> Result<String, String> {
        self.pos += 1;
        let fields = self.parse_obj_type_members(Token::RBrace, "inline obj type")?;
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` to end inline obj type, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let mut name = format!("__inlobj({})", fields.join("|"));
        // V3-18 wedge — `{ ... } | null` shape. Mirror the
        // post-Ident pipe handler for the inline-obj case.
        // An array suffix does not continue past a line break.
        while matches!(self.peek(), Token::LBracket)
            && !self.has_newline_before(self.pos)
            && matches!(
                self.tokens.get(self.pos + 1).map(|s| &s.token),
                Some(Token::RBracket)
            )
        {
            self.pos += 2;
            name.push_str("[]");
        }
        self.consume_union_suffix(name)
    }

    /// The members of an object type body up to (not consuming)
    /// `close`, as `name:T`. A member ends at `,`, `;` or a line break.
    pub(in crate::parser) fn parse_obj_type_members(
        &mut self,
        close: Token,
        what: &str,
    ) -> Result<Vec<String>, String> {
        let mut fields: Vec<String> = Vec::new();
        while *self.peek() != close {
            // `readonly` on a member is type-side only: accepted and
            // discarded.
            if let Token::Ident(s) = self.peek()
                && s == "readonly"
                && let Some(next) = self.tokens.get(self.pos + 1)
                && matches!(next.token, Token::Ident(_))
            {
                self.pos += 1;
            }
            fields.push(self.parse_obj_type_field(what)?);
            match self.peek() {
                Token::Comma | Token::Semi => self.pos += 1,
                t if *t == close => break,
                _ if self.has_newline_before(self.pos) => {}
                t => {
                    return Err(format!(
                        "expected `,` `;` or `}}` in {what}, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        }
        Ok(fields)
    }

    /// One named member `name: T` / `name?: T` / `m(p: T): R`, as
    /// `name:T`.
    fn parse_obj_type_field(&mut self, what: &str) -> Result<String, String> {
        let fname = match self.peek() {
            Token::Ident(n) => n.clone(),
            t => {
                return Err(format!(
                    "expected field name in {what}, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        // V3-18 wedge — optional field `field?: T`. TS spec §3.9:
        // makes the property absence-tolerant, modeled as `T | null`.
        let optional = matches!(self.peek(), Token::Question);
        if optional {
            self.pos += 1;
        }
        // `m(p: T): R` — a MethodSignature, which TS reads as the
        // property holding a `(p: T) => R`, so nothing downstream
        // learns a new shape.
        let fty_raw = if matches!(self.peek(), Token::LParen) {
            self.parse_method_sig_type_ann()?
        } else {
            match self.peek() {
                Token::Colon => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `:` after inline obj field name, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            self.parse_type_ann()?
        };
        // Chunk 793 — struct-field fn slots are Closure-repr. Retag at
        // birth: this is the site that mints `__inlobj(` from syntax,
        // and nested inline objects recurse through here, so every
        // downstream consumer sees the same `__cls(` field the
        // named-TypeDecl lane sees.
        let fty_raw = crate::ast::retag_field_fn_ann(&fty_raw);
        let fty = if optional && !fty_raw.starts_with("__nullable(") {
            format!("__nullable({fty_raw})")
        } else {
            fty_raw
        };
        Ok(format!("{fname}:{fty}"))
    }
}
