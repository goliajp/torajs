//! Param-position OBJECT destructuring — the field walker half of
//! [`super::destr_helpers`], split from it when the §14.3.3
//! ComputedPropertyName arm needed room (the parent sat 9 lines under
//! the 500 limit). The parent keeps the coercible guard, the rest
//! construction, the NamedEvaluation registry, the entry dispatch and
//! the ARRAY walker; this file answers "what lets does an object
//! param pattern become".

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_destr_object_into(
        &mut self,
        src_name: String,
        lets: &mut Vec<Stmt>,
    ) -> Result<(), String> {
        // assumes current token is `{`
        self.pos += 1;
        let guard = self.emit_object_coercible_guard(&src_name);
        lets.push(guard);
        // Keys the pattern names, in source order — the omit set for a
        // trailing `...rest`.
        let mut seen_fields: Vec<PropKey> = Vec::new();
        // The `__ck_N` temps a computed sibling key binds — a trailing
        // rest excludes those by VALUE, having no name to spell.
        let mut computed_keys: Vec<String> = Vec::new();
        if !matches!(self.peek(), Token::RBrace) {
            loop {
                if matches!(self.peek(), Token::DotDotDot) {
                    self.pos += 1;
                    let rest_name = match self.peek() {
                        Token::Ident(n) => n.clone(),
                        t => {
                            return Err(format!(
                                "expected identifier after `...` in object param destructuring, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    };
                    self.pos += 1;
                    let omit: Vec<&str> = seen_fields.iter().filter_map(|f| f.as_str()).collect();
                    let bind = self.emit_obj_rest_let(
                        &src_name,
                        &omit,
                        &computed_keys,
                        &rest_name,
                        false,
                        false,
                    );
                    lets.push(bind);
                    // §14.3.3.1 — a rest element is always last; the
                    // pattern must close here.
                    match self.peek() {
                        Token::RBrace => break,
                        t => {
                            return Err(format!(
                                "rest element must be last in object param destructuring, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                }
                // §14.3.3 ComputedPropertyName — `({ [expr]: binding })`.
                // The field parses through its own method; the shared
                // comma tail below still runs.
                if matches!(self.peek(), Token::LBracket) {
                    let kname = self.parse_destr_computed_field(&src_name, lets)?;
                    computed_keys.push(kname);
                    match self.peek() {
                        Token::Comma => {
                            self.pos += 1;
                            if matches!(self.peek(), Token::RBrace) {
                                break;
                            }
                        }
                        Token::RBrace => break,
                        t => {
                            return Err(format!(
                                "expected `,` or `}}` in object param destructuring, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    continue;
                }
                let (field, field_is_kw) = match self.peek() {
                    Token::Ident(n) => (PropKey::from(n), false),
                    // ES §12.7.2 — escaped ReservedWord names the
                    // FIELD; needs the rename like a bare keyword.
                    Token::EscapedIdent(n) => (PropKey::from(n), true),
                    t if Self::keyword_property_name(t).is_some() => {
                        (PropKey::from(Self::keyword_property_name(t).unwrap()), true)
                    }
                    // §13.3.3 PropertyName : NumericLiteral /
                    // StringLiteral (`{ 0: v }` / `{ "a b": v }`) —
                    // rename mandatory, same as keyword fields; the
                    // load recipe turns all-digit fields into
                    // length-guarded index reads.
                    Token::Number(n) if n.fract() == 0.0 && *n >= 0.0 => {
                        (PropKey::from((*n as u64).to_string()), true)
                    }
                    Token::String(s) => (PropKey::from(s.clone()), true),
                    t => {
                        return Err(format!(
                            "expected identifier in object param destructuring, got {t:?} at {}",
                            self.at()
                        ));
                    }
                };
                self.pos += 1;
                seen_fields.push(field.clone());
                // All-digit fields read as length-guarded index loads
                // (OOB answers undefined so a `= default` wrapper can
                // fire), everything else as a member read.
                let mem = self.dstra_param_field_load(&src_name, &field);
                if matches!(self.peek(), Token::Colon) {
                    self.pos += 1;
                    match self.peek() {
                        Token::Ident(n) => {
                            let nn = n.clone();
                            self.pos += 1;
                            // P-PARSE.3 — `{ x: y = D }`.
                            let init_expr =
                                self.maybe_parse_object_destr_default(mem, Some(&nn))?;
                            lets.push(Stmt::LetDecl {
                                mutable: false,
                                name: nn,
                                type_ann: None,
                                init: init_expr,
                                is_var: false,
                            });
                        }
                        Token::LBracket | Token::LBrace => {
                            // P-PARSE.7 — `{ x: [a, b] = [1, 2] }`.
                            // Mirror the array-destr nested-default
                            // fix from P-PARSE.6: parse the nested
                            // body FIRST so the trailing `=` becomes
                            // visible, then wrap.
                            let nested_id = self.mint_desugar_id();
                            let nested_src = format!("__nested_destr_{nested_id}");
                            let mut nested_body_lets: Vec<Stmt> = Vec::new();
                            self.parse_destr_into(nested_src.clone(), &mut nested_body_lets)?;
                            let init_expr = self.maybe_parse_object_destr_default(mem, None)?;
                            lets.push(Stmt::LetDecl {
                                mutable: false,
                                name: nested_src.clone(),
                                type_ann: None,
                                init: init_expr,
                                is_var: false,
                            });
                            lets.extend(nested_body_lets);
                        }
                        t => {
                            return Err(format!(
                                "expected rename target after `:` in object param destructuring, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                } else {
                    if field_is_kw {
                        let field = field.lossy();
                        return Err(format!(
                            "destructuring field `{field}` is a reserved word; use `{{ {field}: <binding> }}` to rename at {}",
                            self.at()
                        ));
                    }
                    // Shorthand binds the Ident arm's own spelling.
                    let name = field.to_string();
                    let init_expr = self.maybe_parse_object_destr_default(mem, Some(&name))?;
                    lets.push(Stmt::LetDecl {
                        mutable: false,
                        name,
                        type_ann: None,
                        init: init_expr,
                        is_var: false,
                    });
                }
                match self.peek() {
                    Token::Comma => {
                        self.pos += 1;
                        if matches!(self.peek(), Token::RBrace) {
                            break;
                        }
                    }
                    Token::RBrace => break,
                    t => {
                        return Err(format!(
                            "expected `,` or `}}` in object param destructuring, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
            }
        }
        self.pos += 1; // consume `}`
        Ok(())
    }

    /// One `[expr]: binding` field of a param object pattern. The key
    /// hoists into a `__ck_N` temp let (the prologue lets run in field
    /// order at call time, so §14.3.3.3 evaluation order holds), the
    /// load is an any-key index read through the shared
    /// `dstra_computed_load` recipe, and the `:` is mandatory — a
    /// computed key has no shorthand form. Caller sits on the `[`;
    /// returns with the binding consumed, the comma tail untouched.
    /// `__src.f` for a param pattern field: an all-digit field is a
    /// length-guarded index load (OOB answers undefined so a
    /// `= default` wrapper can fire), an identifier string a member
    /// read, and a key that is not a `&str` (lone surrogate) an
    /// index read of the literal, the same routing as `dstra_field_load`.
    fn dstra_param_field_load(&mut self, src_name: &str, field: &PropKey) -> ExprId {
        if let Some(name) = field.as_str() {
            if !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()) {
                let idx = name.parse::<usize>().unwrap_or(0);
                return self.dstra_elem_load(src_name, idx, None);
            }
            let src_ref = self.ast.add_expr(Expr::Ident(src_name.to_string()));
            return self.ast.add_expr(Expr::Member {
                obj: src_ref,
                name: name.to_string(),
            });
        }
        let src_ref = self.ast.add_expr(Expr::Ident(src_name.to_string()));
        let key = self.ast.add_expr(Expr::String(field.as_wtf8().to_owned()));
        self.ast.add_expr(Expr::Index {
            obj: src_ref,
            index: key,
        })
    }

    /// Answers the `__ck_N` temp it bound, so a trailing rest can
    /// exclude the key by value.
    fn parse_destr_computed_field(
        &mut self,
        src_name: &str,
        lets: &mut Vec<Stmt>,
    ) -> Result<String, String> {
        self.pos += 1; // consume `[`
        let key_expr = self.with_yield_hoist_disallowed(|p| p.parse_assign())?;
        match self.peek() {
            Token::RBracket => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `]` after computed key in object param destructuring, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        match self.peek() {
            Token::Colon => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `:` after computed key in object param destructuring, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let id = self.mint_desugar_id();
        let kname = format!("__ck_{id}");
        // §13.15.5.5 converts once, here — a sibling rest then
        // excludes the CONVERTED key without asking `toString` twice.
        let init = self.wrap_to_property_key(key_expr);
        lets.push(Stmt::LetDecl {
            mutable: false,
            name: kname.clone(),
            type_ann: None,
            init,
            is_var: false,
        });
        let mem = self.dstra_computed_load(src_name, &kname, None);
        match self.peek() {
            Token::Ident(n) => {
                let nn = n.clone();
                self.pos += 1;
                let init_expr = self.maybe_parse_object_destr_default(mem, Some(&nn))?;
                lets.push(Stmt::LetDecl {
                    mutable: false,
                    name: nn,
                    type_ann: None,
                    init: init_expr,
                    is_var: false,
                });
            }
            Token::LBracket | Token::LBrace => {
                // Mirror the static arm's P-PARSE.7 order: parse the
                // nested body FIRST so a trailing `=` becomes visible,
                // then wrap the load in the default.
                let nested_id = self.mint_desugar_id();
                let nested_src = format!("__nested_destr_{nested_id}");
                let mut nested_body_lets: Vec<Stmt> = Vec::new();
                self.parse_destr_into(nested_src.clone(), &mut nested_body_lets)?;
                let init_expr = self.maybe_parse_object_destr_default(mem, None)?;
                lets.push(Stmt::LetDecl {
                    mutable: false,
                    name: nested_src,
                    type_ann: None,
                    init: init_expr,
                    is_var: false,
                });
                lets.extend(nested_body_lets);
            }
            t => {
                return Err(format!(
                    "expected rename target after computed key in object param destructuring, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        Ok(kname)
    }
}
