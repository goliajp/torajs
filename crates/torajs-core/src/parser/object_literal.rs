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
        // `async [computedKey]() {}` — computed-key form stays on the
        // pre-existing stub-drop path (real substrate gated on Symbol.X
        // dispatch, P3/P7 follow-up). Body brace-balanced + emit
        // `__async_<sym>: null`.
        if matches!(self.peek(), Token::Async)
            && let Some(t1) = self.tokens.get(self.pos + 1)
            && matches!(t1.token, Token::LBracket)
        {
            self.pos += 2; // consume `async` + `[`
            let key = match self.peek() {
                Token::String(s) => {
                    let k = s.clone();
                    self.pos += 1;
                    k
                }
                Token::Ident(_) => {
                    let mut parts: Vec<String> = Vec::new();
                    while let Token::Ident(n) = self.peek() {
                        parts.push(n.clone());
                        self.pos += 1;
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
                        "async [<key>]: expected key, got {t:?} at {}",
                        self.at()
                    ));
                }
            };
            if matches!(self.peek(), Token::RBracket) {
                self.pos += 1;
            }
            let synth_name = format!("__async_{key}");
            if matches!(self.peek(), Token::LParen) {
                self.pos += 1;
                let mut depth = 1i32;
                while depth > 0 {
                    match self.peek() {
                        Token::LParen => depth += 1,
                        Token::RParen => depth -= 1,
                        Token::Eof => {
                            return Err(format!(
                                "unexpected eof in async method params at {}",
                                self.at()
                            ));
                        }
                        _ => {}
                    }
                    self.pos += 1;
                }
            }
            if matches!(self.peek(), Token::Colon) {
                self.pos += 1;
                let _ = self.parse_type_ann()?;
            }
            if matches!(self.peek(), Token::LBrace) {
                self.pos += 1;
                let mut depth = 1i32;
                while depth > 0 {
                    match self.peek() {
                        Token::LBrace => depth += 1,
                        Token::RBrace => depth -= 1,
                        Token::Eof => {
                            return Err(format!(
                                "unexpected eof in async method body at {}",
                                self.at()
                            ));
                        }
                        _ => {}
                    }
                    self.pos += 1;
                }
            }
            let value = self.ast.add_expr(Expr::Null);
            return Ok((synth_name, value));
        }
        // V3-18 P2.4.c.4 — computed property `{ [key]: value }` per
        // JS spec. Subset only supports literal-string keys at compile
        // time (struct layouts are static); runtime keys defer to a
        // dictionary substrate. `{ [<StringLit>]: v }` rewrites to
        // `{ <StringLit>: v }`.
        // Symbol.X / member-shape keys are parsed but get a synthetic
        // name `__sym_<accessor>__` so downstream layout works; the
        // real iterator-protocol dispatch lands with Phase E.
        if matches!(self.peek(), Token::LBracket) {
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
                return Ok((key, value));
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
            return Ok((key, value));
        }
        let name = match self.peek() {
            Token::Ident(n) => n.clone(),
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
        if (name == "get" || name == "set")
            && matches!(
                self.peek(),
                Token::Ident(_) | Token::String(_) | Token::Number(_)
            )
        {
            let kind = name.clone();
            // P0.10 — getter / setter shorthand also accepts string-
            // literal and numeric-literal property names per ES spec
            // §12.7.6 PropertyName. Pre-fix only Ident was accepted.
            let prop_name = match self.peek() {
                Token::Ident(n) => n.clone(),
                Token::String(s) => s.clone(),
                Token::Number(n) => crate::ast::number_prop_key(*n),
                _ => unreachable!(),
            };
            self.pos += 1;
            if matches!(self.peek(), Token::LParen) {
                // RFC 20260714-objlit-accessor blade 2 — parse the body
                // for real. It used to be walked brace-balanced and
                // THROWN AWAY, leaving a `__getter_<n>: null` placeholder
                // field: the accessor never ran, and even a direct read
                // `o.b` failed ("no member `.b` on Struct([(\"__getter_b\",
                // Null)])"). The stated reason was that a getter body
                // uses `this`, which only resolved inside a class method
                // — blade 1 fixed that, so the body can be a normal
                // method now.
                //
                // The value is an ArrowFn under the same `__getter_<n>` /
                // `__setter_<n>` synth name, marked as a method so it
                // picks up the `__this` receiver and the `__mth(` ABI.
                // Keeping the accessor IN the layout is what makes the
                // type carry it: `{a:1, get b(){}}` is structurally
                // distinct from `{a:1}`, so no same-layout object can
                // reach for its getter (RFC §2.1).
                let (params, destr_lets) = self.parse_param_list()?;
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
                            "expected `{{` after {kind}ter `{prop_name}` header, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
                let mut body = Vec::new();
                while !matches!(self.peek(), Token::RBrace | Token::Eof) {
                    body.push(self.parse_stmt()?);
                }
                match self.peek() {
                    Token::RBrace => self.pos += 1,
                    t => {
                        return Err(format!(
                            "expected `}}` after {kind}ter `{prop_name}` body, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
                let body = if destr_lets.is_empty() {
                    body
                } else {
                    let mut full = destr_lets;
                    full.extend(body);
                    full
                };
                let value = self.ast.add_expr(Expr::ArrowFn {
                    params,
                    return_type,
                    body,
                });
                self.ast.objlit_method_exprs.insert(value);
                let synth = format!("__{kind}ter_{prop_name}");
                return Ok((synth, value));
            }
            // get / set followed by ident but not by `(` — treat as
            // regular field (the ident-after path will hit the
            // expected-`:` error like before).
        }
        // Method shorthand: `{ valueOf() { ... } }` is sugar for
        // `{ valueOf: function () { ... } }`. The parser was rejecting
        // these with "expected `:`, got LParen" — accept the shorthand
        // by routing through `parse_fn_expr`-equivalent shape, then
        // sticking the resulting `Expr::ArrowFn` under the field name.
        if matches!(self.peek(), Token::LParen) {
            let (mut params, destr_lets) = self.parse_param_list()?;
            // 刀 1b — method-position default params infer their ann
            // from the default (see param_list.rs).
            self.infer_default_param_anns(&mut params);
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
                        "expected `{{` after method shorthand `{name}` header, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            let mut body = Vec::new();
            while !matches!(self.peek(), Token::RBrace | Token::Eof) {
                body.push(self.parse_stmt()?);
            }
            match self.peek() {
                Token::RBrace => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `}}` after method shorthand `{name}` body, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            let body = if destr_lets.is_empty() {
                body
            } else {
                let mut full = destr_lets;
                full.extend(body);
                full
            };
            let value = self.ast.add_expr(Expr::ArrowFn {
                params,
                return_type,
                body,
            });
            // RFC 20260714-objlit-accessor blade 1 — the ArrowFn node is
            // a lossy encoding of a method: an arrow takes the LEXICAL
            // `this`, a method binds it to the receiver. Record the
            // method position so `desugar_objlit_nominal` can give this
            // closure a `__this` param — without the mark, the `this` in
            // the body is left a free variable and the checker rejects it
            // ("references unknown identifier `__this`").
            self.ast.objlit_method_exprs.insert(value);
            return Ok((name, value));
        }
        // Property shorthand: `{ x }` is sugar for `{ x: x }`. Triggers
        // when the field name isn't followed by `:` AND isn't followed
        // by `(` (the method shorthand path above).
        if matches!(self.peek(), Token::Comma | Token::RBrace) {
            let value = self.ast.add_expr(Expr::Ident(name.clone()));
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
        Ok((name, value))
    }
}
