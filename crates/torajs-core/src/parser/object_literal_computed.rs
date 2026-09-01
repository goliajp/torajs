//! Computed-property arm of the object-literal parser — split out as
//! the rotation-197 file-size sweep. The parent `object_literal.rs`
//! drifted to 507 prod LOC after the `{ async [computedKey]() {} }`
//! stub-drop + `{ [expr]() {} }` method-shorthand wedges landed on
//! top of the original literal-string / Symbol.X-chain key path.
//!
//! Covers V3-18 P2.4.c.4 + P0.10 + RFC 20260725-objlit-computed-key:
//! - `{ ["k"]: v }` (the string IS the whole key) still folds to
//!   `{ k: v }` at parse time.
//! - Every other key is a RUNTIME computed key (刀 1): the expr
//!   parses for real, the field name is a `__computed_<n>__`
//!   sentinel, and `ast.objlit_computed_keys` maps value → key expr
//!   for the dynobj-init lane's ToPropertyKey evaluation. Method
//!   shorthand bodies parse through the shared accessor/method tail.
//!
//! RFC 20260725-getiterator-getmethod 刀 1 — a `Symbol.<chain>` key
//! used to be a third case, folded at parse time into a
//! `__sym_<chain>__` NAME. That encoding was a dead end: the only
//! consumers of `__sym_…` are the class-side vtable and fn_table
//! lookups, so an object literal's mangled key answered nothing,
//! reported 0 own symbols, and leaked a fake string property into
//! `getOwnPropertyNames`. It is a computed key like any other now, so
//! §7.1.19 ToPropertyKey hands the dynobj-init lane a real Symbol —
//! and its method shorthand parses its body for real instead of
//! dropping it on the floor.

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
        is_async: bool,
    ) -> Result<Option<(String, ExprId)>, String> {
        if !matches!(self.peek(), Token::LBracket) {
            return Ok(None);
        }
        let member_start_pos = self.pos;
        self.pos += 1;
        let key = match self.peek() {
            // A literal-string key folds only when the string IS the
            // whole key (`["k"]`); a composite expression starting
            // with a string (`["a" + b]`) is a runtime computed key.
            Token::String(s) if matches!(self.tokens[self.pos + 1].token, Token::RBracket) => {
                let key = s.to_string_lossy_owned();
                self.pos += 1;
                key
            }
            _ => {
                return self
                    .parse_runtime_computed_property(member_start_pos, is_async)
                    .map(Some);
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
            self.mark_computed_proto_own(&key, value);
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
        self.mark_fnexpr_field_method(value);
        self.mark_computed_proto_own(&key, value);
        Ok(Some((key, value)))
    }

    /// RFC 20260725-objlit-computed-key 刀 1 — a computed key that is
    /// neither a literal string nor a `Symbol.`-chain parses as a
    /// real expression. The field name is a unique
    /// `__computed_<n>__` sentinel and the key expr registers in
    /// `objlit_computed_keys`; the dynobj-init lane evaluates it
    /// (ToPropertyKey string face) in field order. A method
    /// shorthand body parses for real through the shared
    /// accessor/method tail — no more stub-drop.
    fn parse_runtime_computed_property(
        &mut self,
        member_start_pos: usize,
        is_async: bool,
    ) -> Result<(String, ExprId), String> {
        let key_expr = self.parse_assign()?;
        match self.peek() {
            Token::RBracket => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `]` after computed property key, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let name = format!("__computed_{}__", self.ast.objlit_computed_keys.len());
        let value = if matches!(self.peek(), Token::LParen) {
            let v = self.parse_method_like_value_async(
                member_start_pos,
                false,
                "computed-key method shorthand",
                is_async,
            )?;
            if is_async {
                self.ast.async_fn_value_exprs.insert(v);
            }
            v
        } else {
            match self.peek() {
                Token::Colon => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `:` after `[<key>]` in object literal, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            let v = self.parse_assign()?;
            self.mark_fnexpr_field_method(v);
            v
        };
        self.ast.objlit_computed_keys.insert(value, key_expr);
        Ok((name, value))
    }

    /// RFC 20260725 (fn-expr field this), computed-key twin — a plain
    /// `function` expression standing as a computed property's VALUE
    /// binds `this` to the call-site receiver exactly like the
    /// name-keyed field (`parse_object_field`'s marking, which this
    /// mirrors verbatim). A computed-key literal always rides the
    /// dynobj lane (anylane leg (g)), so the promoted body takes the
    /// `__this: any` receiver-first shape — the same path the
    /// computed-key method shorthand already proves out. Async forms
    /// keep the recorded loud boundary (fnexpr_this module doc).
    fn mark_fnexpr_field_method(&mut self, value: ExprId) {
        if self.ast.fn_expr_exprs.contains(&value)
            && !self.ast.async_fn_value_exprs.contains(&value)
        {
            self.ast.objlit_method_exprs.insert(value);
        }
    }

    /// P-SURF S2.27 — computed accessor name: `{ get [expr]() {} }` /
    /// `{ set [expr](v) {} }` (§13.2.5 ComputedPropertyName in an
    /// accessor MethodDefinition). Caller has consumed the `get`/`set`
    /// keyword and sits on the `[`. A literal-string whole key folds
    /// to the static `__getter_<k>` / `__setter_<k>` slot (same fold
    /// as `try_parse_computed_property`); every other key parses as a
    /// runtime computed key — sentinel field name, key expr in
    /// `objlit_computed_keys`, accessor kind in
    /// `objlit_computed_accessors` so the dynobj-init computed arm
    /// routes the face through the accessor define kernel.
    pub(super) fn parse_computed_accessor(
        &mut self,
        kind: &str,
        member_start_pos: usize,
    ) -> Result<(String, ExprId), String> {
        self.pos += 1;
        if let Token::String(s) = self.peek()
            && matches!(self.tokens[self.pos + 1].token, Token::RBracket)
        {
            let prop = s.to_string_lossy_owned();
            self.pos += 2;
            let value = self.parse_method_like_value(
                member_start_pos,
                false,
                &format!("{kind}ter `{prop}`"),
            )?;
            return Ok((format!("__{kind}ter_{prop}"), value));
        }
        let key_expr = self.parse_assign()?;
        match self.peek() {
            Token::RBracket => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `]` after computed accessor key, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let value = self.parse_method_like_value(
            member_start_pos,
            false,
            &format!("computed-key {kind}ter"),
        )?;
        let name = format!("__computed_{}__", self.ast.objlit_computed_keys.len());
        self.ast.objlit_computed_keys.insert(value, key_expr);
        self.ast
            .objlit_computed_accessors
            .insert(value, kind == "get");
        Ok((name, value))
    }

    /// §B.3.1 step 5 — the `__proto__: v` [[Prototype]]-set special
    /// case requires `IsComputedPropertyKey(propKey)` to be FALSE: a
    /// computed `['__proto__']` key defines an ordinary own property.
    /// The parse-time fold to a plain field name would otherwise hit
    /// the dynobj-init proto arm, so record the value expr in the
    /// own-property side channel (shared with the `{ __proto__ }`
    /// shorthand, which needs the identical skip).
    fn mark_computed_proto_own(&mut self, key: &str, value: ExprId) {
        if key == "__proto__" {
            self.ast.objlit_shorthand_proto_exprs.insert(value);
        }
    }
}
