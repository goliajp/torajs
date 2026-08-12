//! Object-literal cluster (chunk 413).
//!
//! Extracted verbatim from parser.rs — the three methods that parse
//! `{ ... }` object-literal expressions:
//! - parse_object_literal — `{ f1: e1, f2: e2, ... }` driver with
//!   trailing-comma tolerance
//! - parse_object_field_or_spread — one member: `...src` spread
//!   (sentinel field name `__spread__`) or a regular field
//! - parse_object_field — the field grammar: async-method shorthand
//!   (via object_member sibling), `async [computedKey]() {}` stub-drop,
//!   computed `[key]: v` / `[key]() {}`, keyword / string / numeric
//!   property names, getter-setter shorthand (`__getter_x` /
//!   `__setter_x` synth names), method shorthand (`name() {}` →
//!   ArrowFn), property shorthand (`{ x }` → `{ x: x }`)
//!
//! parse_object_literal is called from parse_primary (parser.rs);
//! the other two are internal to this cluster. All promoted
//! `pub(super)` per the sibling-impl pack pattern. Body unchanged.

use super::*;

impl<'a> Parser<'a> {
    /// `{ name: expr, ... }` — assumes current token is `{`.
    pub(super) fn parse_object_literal(&mut self) -> Result<ExprId, String> {
        self.pos += 1; // consume `{`
        let mut fields: Vec<(String, ExprId)> = Vec::new();
        if !matches!(self.peek(), Token::RBrace) {
            fields.push(self.parse_object_field_or_spread()?);
            while matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                if matches!(self.peek(), Token::RBrace) {
                    break; // trailing comma
                }
                fields.push(self.parse_object_field_or_spread()?);
            }
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` in object literal, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        Ok(self.ast.add_expr(Expr::ObjectLit { fields }))
    }

    /// One member inside an object literal — either `name: expr` or
    /// `...src` spread. Spread is encoded with the sentinel field name
    /// `__spread__` so the existing `Vec<(String, ExprId)>` shape
    /// doesn't need to change.
    pub(super) fn parse_object_field_or_spread(&mut self) -> Result<(String, ExprId), String> {
        if matches!(self.peek(), Token::DotDotDot) {
            self.pos += 1;
            let inner = self.parse_expr()?;
            return Ok(("__spread__".to_string(), inner));
        }
        self.parse_object_field()
    }

    /// One `name: expr` pair inside an object literal.
    pub(super) fn parse_object_field(&mut self) -> Result<(String, ExprId), String> {
        // P10.3-A3b — `{ async name() { ... } }` real substrate. See
        // `parser/object_member.rs`. Returns None when the leading
        // token isn't an async-method shape (caller falls through to
        // computed-key stub-drop below or regular field paths).
        if let Some(pair) = self.try_parse_async_object_method_shorthand()? {
            return Ok(pair);
        }
        // RFC 20260809 knife 3 residue — `{ async [key]() {} }` parses
        // for REAL through the computed-property arm (the old
        // stub-drop emitted `__async_<sym>: null` and threw the body
        // away — an `async [Symbol.asyncDispose]()` resource answered
        // undefined). Consume the `async`, hand the arm an async
        // body context.
        if matches!(self.peek(), Token::Async)
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.token),
                Some(Token::LBracket)
            )
        {
            self.pos += 1; // consume `async`
            if let Some(pair) = self.try_parse_computed_property(true)? {
                return Ok(pair);
            }
            return Err(format!(
                "expected `[` after `async` in object literal at {}",
                self.at()
            ));
        }
        // P-SURF S2.1 — `{ *g() { yield 1 } }`. See
        // `parser/object_member_generator.rs`; returns None unless the
        // lookahead is `* <Ident> (`, so `*` in any other position keeps
        // its existing diagnostic.
        if let Some(pair) = self.try_parse_generator_object_method()? {
            return Ok(pair);
        }
        if let Some(pair) = self.try_parse_computed_property(false)? {
            return Ok(pair);
        }
        let name = match self.peek() {
            Token::Ident(n) => n.clone(),
            // ES §12.7.2 — an escaped ReservedWord is a legal
            // IdentifierName here (`{ bre\u0061k: 1 }`).
            Token::EscapedIdent(n) => n.clone(),
            // V3-18 wedge — accept reserved-word tokens as object-
            // literal field names per ES spec §12.7.6 (the full
            // reserved-word set is allowed in property-name
            // positions). Pre-fix `{ type: ... }`, `{ default: ... }`,
            // etc. all bailed at "expected field name".
            t if Self::keyword_property_name(t).is_some() => {
                Self::keyword_property_name(t).unwrap().to_string()
            }
            // P0.10 — string-literal property name `{ "0": ... }` /
            // `{ "key": ... }` per ES spec §12.7.6 PropertyName ::
            // StringLiteral. Used pervasively in test262 (~10+ cases
            // directly + many transitively for object-with-string-
            // keys patterns).
            Token::String(s) => s.clone(),
            // P0.10 — numeric-literal property name `{ 0: ... }` /
            // `{ 99: ... }` per ES spec §12.7.6 PropertyName ::
            // NumericLiteral. Massive yield — 600+ test262 cases use
            // numeric-key object literals (e.g. `{ 0: arr[0], 1: ... }`
            // for spread-iter style fixtures). Spelling shared with
            // the chunk-745 struct-index lanes via
            // [`crate::ast::number_prop_key`].
            Token::Number(n) => crate::ast::number_prop_key(*n),
            t => {
                return Err(format!(
                    "expected field name in object literal, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        // Span anchor for the accessor / method-shorthand ArrowFns
        // below — the member-name token (`get` / `set` / the method
        // name), which is where their source text starts (RFC
        // 20260719-fn-tostring-source B1).
        let member_start_pos = self.pos - 1;
        // P-PARSE.4 — getter / setter shorthand `{ get NAME() {...} }`
        // / `{ set NAME(v) {...} }` per ES spec §12.7.6. Pre-fix the
        // parser saw `get` as a regular field name and bailed at the
        // following `NAME` ident with 'expected `:` after field name
        // `get`'. Test262's language/expressions/array/spread-obj-*
        // suite uses these pervasively.
        //
        // The parser accepts the syntax and stashes the body so the
        // surrounding obj literal still constructs. tora has no real
        // accessor-descriptor substrate yet (P3 / P7), so the
        // synthesised field name encodes the kind:
        //   `get x() { ... }`   →  `__getter_x: () => { ... }`
        //   `set x(v) { ... }`  →  `__setter_x: (v) => { ... }`
        // This isn't spec-correct accessor semantics — `o.x` won't
        // call the getter, the function value sits in `__getter_x`
        // instead. But the parse succeeds and the surrounding obj
        // literal compiles, which is what P-PARSE.4 needs. Test262
        // cases that assert parse acceptance (vs accessor behaviour)
        // start passing; cases that depend on the accessor semantic
        // remain blocked until P3 / P7 lands.
        if let Some(pair) = self.try_parse_accessor_shorthand(&name, member_start_pos)? {
            return Ok(pair);
        }
        // Method shorthand: `{ valueOf() { ... } }` is sugar for
        // `{ valueOf: function () { ... } }`. The parser was rejecting
        // these with "expected `:`, got LParen" — accept the shorthand
        // by routing through `parse_fn_expr`-equivalent shape, then
        // sticking the resulting `Expr::ArrowFn` under the field name.
        // 刀 1b — method-position default params infer their ann from
        // the default (see param_list.rs), hence infer_defaults: true.
        if matches!(self.peek(), Token::LParen) {
            let value = self.parse_method_like_value(
                member_start_pos,
                true,
                &format!("method shorthand `{name}`"),
            )?;
            return Ok((name, value));
        }
        // Property shorthand: `{ x }` is sugar for `{ x: x }`. Triggers
        // when the field name isn't followed by `:` AND isn't followed
        // by `(` (the method shorthand path above).
        if matches!(self.peek(), Token::Comma | Token::RBrace) {
            // The shorthand is the one place an object literal spells
            // an IdentifierReference rather than a property NAME, so it
            // is the one place §12.7.2 applies: `{ static: 1 }` and
            // `{ static() {} }` name a property and stay legal in
            // strict code, `{ static }` reads a variable and does not.
            // The recorded offset is the delimiter rather than the name
            // itself — the name has already been consumed by the time
            // the shorthand shape is recognised, and the message says
            // which word.
            self.note_strict_reference(&name)?;
            let value = self.ast.add_expr(Expr::Ident(name.clone()));
            // §B.3.1 — a shorthand `__proto__` is an ordinary own
            // property; only the `__proto__: v` production sets
            // [[Prototype]]. Mark it so the dynobj-init lane skips
            // the proto special-case.
            if name == "__proto__" {
                self.ast.objlit_shorthand_proto_exprs.insert(value);
            }
            return Ok((name, value));
        }
        // S2.24 刀 4 — CoverInitializedName `{ x = D }` (§13.2.5.1):
        // legal ONLY when the literal is re-read as an assignment
        // pattern. Read it the way the cover grammar does — the field
        // value is the assignment expression `x = D`, exactly the
        // shape the pattern walk's `f: y = D` default arm consumes —
        // and record the eid; a literal that survives to expression
        // position early-errors on it (check_type_of_object_lit).
        if matches!(self.peek(), Token::Eq) {
            // Same reference position as the plain shorthand above, and
            // an assignment target on top of it — either clause refuses
            // the seven words, so the one judge answers both.
            self.note_strict_reference(&name)?;
            self.pos += 1;
            let default = self.parse_expr()?;
            let target = self.ast.add_expr(Expr::Ident(name.clone()));
            let value = self.ast.add_expr(Expr::Assign {
                target,
                value: default,
            });
            self.ast.objlit_cover_init_exprs.insert(value);
            return Ok((name, value));
        }
        match self.peek() {
            Token::Colon => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `:` after field name `{name}`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let value = self.parse_expr()?;
        // RFC 20260725 (fn-expr field this) — a plain `function`
        // expression in a field position binds `this` to the
        // call-site receiver exactly like the method shorthand (the
        // shorthand is its sugar, ES §10.2.1.2), so it joins
        // `objlit_method_exprs` and rides objlit_nominal's `__this`
        // promotion. The uses-this gate there keeps a this-free
        // fn-expr on the plain closure ABI; async forms keep the
        // recorded loud boundary (fnexpr_this module doc).
        if self.ast.fn_expr_exprs.contains(&value)
            && !self.ast.async_fn_value_exprs.contains(&value)
        {
            self.ast.objlit_method_exprs.insert(value);
        }
        Ok((name, value))
    }

    /// P-PARSE.4 — getter / setter shorthand `{ get NAME() {...} }` /
    /// `{ set NAME(v) {...} }` per ES spec §12.7.6.
    ///
    /// RFC 20260714-objlit-accessor blade 2 — the body is parsed for
    /// real (it used to be walked brace-balanced and thrown away,
    /// leaving a `__getter_<n>: null` placeholder that broke even a
    /// direct read). The value is an ArrowFn under the `__getter_<n>` /
    /// `__setter_<n>` synth name, marked as a method so it picks up the
    /// `__this` receiver and the `__mth(` ABI. Keeping the accessor IN
    /// the layout is what makes the type carry it: `{a:1, get b(){}}`
    /// is structurally distinct from `{a:1}` (RFC §2.1).
    ///
    /// Returns `None` when `name` isn't `get`/`set` followed by a
    /// property name. NOTE: `get`/`set` followed by a property name but
    /// NOT by `(` also returns `None` — with the property-name token
    /// already consumed, so the caller's `:`-check path reports the
    /// same "expected `:` after field name `get`" error as before.
    fn try_parse_accessor_shorthand(
        &mut self,
        name: &str,
        member_start_pos: usize,
    ) -> Result<Option<(String, ExprId)>, String> {
        if !((name == "get" || name == "set")
            && (matches!(
                self.peek(),
                Token::Ident(_) | Token::String(_) | Token::Number(_) | Token::LBracket
            ) || Self::keyword_property_name(self.peek()).is_some()))
        {
            return Ok(None);
        }
        // P-SURF S2.27 — computed accessor name `{ get [expr]() {} }` /
        // `{ set [expr](v) {} }` per §13.2.5 ComputedPropertyName in a
        // MethodDefinition accessor. See `object_literal_computed.rs`.
        if matches!(self.peek(), Token::LBracket) {
            let pair = self.parse_computed_accessor(name, member_start_pos)?;
            self.reject_objlit_accessor_arity(name, pair.1)?;
            return Ok(Some(pair));
        }
        // P0.10 — getter / setter shorthand also accepts string-
        // literal and numeric-literal property names per ES spec
        // §12.7.6 PropertyName. Pre-fix only Ident was accepted.
        let prop_name = match self.peek() {
            Token::Ident(n) => n.clone(),
            Token::String(s) => s.clone(),
            Token::Number(n) => crate::ast::number_prop_key(*n),
            // §12.7.6 — the full reserved-word set is legal in
            // property-name positions (`{ get yield() {} }`); the
            // gate above admitted it via the shared table.
            t => Self::keyword_property_name(t).unwrap().to_string(),
        };
        self.pos += 1;
        if !matches!(self.peek(), Token::LParen) {
            return Ok(None);
        }
        let value = self.parse_method_like_value(
            member_start_pos,
            false,
            &format!("{name}ter `{prop_name}`"),
        )?;
        self.reject_objlit_accessor_arity(name, value)?;
        let synth = format!("__{name}ter_{prop_name}");
        Ok(Some((synth, value)))
    }

    /// §15.4.1 early errors for the object-literal accessor
    /// shorthands — the parsed body is an `Expr::ArrowFn` whose params
    /// are still the source parameter list (no desugar has run yet);
    /// non-ArrowFn stubs (async computed) pass through.
    fn reject_objlit_accessor_arity(&self, name: &str, value: ExprId) -> Result<(), String> {
        if let Expr::ArrowFn { params, .. } = self.ast.get_expr(value) {
            let kind = if name == "get" {
                ast::AccessorKind::Getter
            } else {
                ast::AccessorKind::Setter
            };
            self.reject_accessor_arity(Some(kind), params, &format!("{name}ter"))?;
        }
        Ok(())
    }

    /// Shared tail of the accessor / method shorthands:
    /// `(params) [: ret] { body }` → an `Expr::ArrowFn` anchored at
    /// `member_start_pos` (the member-name token, where the source
    /// text starts — RFC 20260719-fn-tostring-source B1).
    ///
    /// RFC 20260714-objlit-accessor blade 1 — the ArrowFn node is a
    /// lossy encoding of a method: an arrow takes the LEXICAL `this`,
    /// a method binds it to the receiver. Registering the ExprId in
    /// `objlit_method_exprs` is what lets `desugar_objlit_nominal`
    /// give the closure a `__this` param — without the mark, `this`
    /// in the body is left a free variable and the checker rejects it.
    pub(super) fn parse_method_like_value(
        &mut self,
        member_start_pos: usize,
        infer_defaults: bool,
        err_ctx: &str,
    ) -> Result<ExprId, String> {
        self.parse_method_like_value_async(member_start_pos, infer_defaults, err_ctx, false)
    }

    /// [`Self::parse_method_like_value`] with an async-body context:
    /// `await` is legal inside (§15.8.1) — the `{ async [key]() {} }`
    /// computed form (knife 3 residue; the caller registers the value
    /// in `async_fn_value_exprs`).
    pub(super) fn parse_method_like_value_async(
        &mut self,
        member_start_pos: usize,
        infer_defaults: bool,
        err_ctx: &str,
        is_async: bool,
    ) -> Result<ExprId, String> {
        let (mut params, destr_lets) = self.parse_param_list()?;
        // Method shorthand and accessors are method definitions.
        self.reject_duplicate_params(&params, true)?;
        if infer_defaults {
            self.infer_default_param_anns(&mut params);
        }
        let return_type = if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            Some(self.parse_type_ann()?)
        } else {
            None
        };
        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` after {err_ctx} header, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // S2.9 — an object-literal method has a `[[HomeObject]]` but no
        // constructor role, so `super()` in it is an early SyntaxError
        // (ES §15.7.1) even when the literal sits in a derived ctor.
        let saved_super = std::mem::replace(&mut self.super_call_allowed, false);
        // r334 blade 6 — an object-literal method DOES have a
        // [[HomeObject]], so SuperProperty parses here (§15.4.1). tr
        // cannot lower it yet (the home's prototype is a runtime value
        // — L3b home-object channel), so the marker survives to the
        // checker and fails loud rather than being refused at parse.
        let saved_super_prop = std::mem::replace(&mut self.super_prop_allowed, true);
        // §15.8.1 — a non-async method/accessor body may not await;
        // an async one may.
        let saved_await = std::mem::replace(&mut self.await_allowed, is_async);
        let strict_outer = self.in_strict_fn;
        let mut body = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            match self.parse_stmt() {
                Ok(s) => {
                    self.arm_strict_directive(&s, &body);
                    body.push(s);
                }
                Err(e) => {
                    self.await_allowed = saved_await;
                    self.super_call_allowed = saved_super;
                    self.super_prop_allowed = saved_super_prop;
                    return Err(e);
                }
            }
        }
        self.await_allowed = saved_await;
        self.super_call_allowed = saved_super;
        self.super_prop_allowed = saved_super_prop;
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` after {err_ctx} body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        self.reject_lexical_shadowing_param(&params, &destr_lets, &body)?;
        self.reject_use_strict_with_non_simple_params(&params, &body)?;
        self.finish_fn_body_strict(strict_outer, &params, &mut body)?;
        let body = if destr_lets.is_empty() {
            body
        } else {
            let mut full = destr_lets;
            full.extend(body);
            full
        };
        let value = self.add_expr_at(
            member_start_pos,
            Expr::ArrowFn {
                params,
                return_type,
                body,
            },
        );
        self.ast.objlit_method_exprs.insert(value);
        Ok(value)
    }
}
