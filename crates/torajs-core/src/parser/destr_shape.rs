//! Declaration-position destructuring — the recursive PatShape
//! machine (RFC 20260727-dstr-decl-shape).
//!
//! One reader + one emitter serve every declaration-position pattern
//! (`let [..] = src`, `let {..} = src`, `for (let [..] of src)`),
//! replacing the three flat walks that each accepted a different
//! slice of the grammar (statement walks refused nested patterns;
//! the for-of scan refused everything but bare names). The
//! assignment-position machine (dstr_assign.rs, S2.24) stays its own
//! walk — its slots are assignment targets, not fresh bindings — but
//! the two share the load recipes (`dstra_elem_load` /
//! `dstra_field_load`) so the §13.3.3 default semantics can never
//! drift between lanes.
//!
//! Recorded boundaries (loud, never silent): `for (let PAT in obj)`
//! pattern heads; CoverInitializedName in assignment-position object
//! heads. Object rest alongside a computed key used to be one — the
//! omit-set protocol carries static names only — and is not any more:
//! a computed sibling sends the rest through `__torajs_obj_rest`
//! (`destr_helpers::emit_obj_rest_call`), which carries the runtime
//! keys as values.

use super::*;

/// One parsed declaration pattern, nesting included.
pub(super) enum PatShape {
    Ary {
        elems: Vec<AryElem>,
        rest: Option<RestShape>,
    },
    Obj {
        fields: Vec<ObjField>,
        rest: Option<String>,
    },
}

/// An array pattern's rest slot — §13.3.3 BindingRestElement is
/// `... BindingIdentifier` or `... BindingPattern` (`[...[x, y]]`).
/// Object rest stays a bare String: BindingRestProperty admits only
/// an identifier.
pub(super) enum RestShape {
    Bind(String),
    Nested(Box<PatShape>),
}

pub(super) enum AryElem {
    /// `,,` — position advances, nothing binds.
    Elide,
    Bind {
        name: String,
        default: Option<ExprId>,
    },
    Nested {
        pat: PatShape,
        default: Option<ExprId>,
    },
}

pub(super) struct ObjField {
    pub(super) key: FieldKey,
    pub(super) binding: ObjBinding,
}

/// An object field's property name — §14.3.3 PropertyName is either a
/// static name (identifier / keyword / numeric / string literal, all
/// folded to a String at parse time) or a `[expr]`
/// ComputedPropertyName. A computed key stays a raw expression here;
/// the emitter evaluates it in field order (§14.3.3.3 binds each
/// field's key before its load, after the previous field's bind).
pub(super) enum FieldKey {
    Named(PropKey),
    Computed(ExprId),
}

pub(super) enum ObjBinding {
    Bind {
        name: String,
        default: Option<ExprId>,
    },
    Nested {
        pat: PatShape,
        default: Option<ExprId>,
    },
}

impl PatShape {
    /// §8.2.1 BoundNames of a BindingPattern — every identifier the
    /// pattern declares, nesting and rest slots included, in source
    /// order. A default initializer binds nothing of its own, so the
    /// walk never enters one.
    ///
    /// The catch clause needs this: §13.15.1 judges the BoundNames of
    /// the CatchParameter against the block's LexicallyDeclaredNames,
    /// and a destructured parameter binds through a synthesized
    /// `__catch_destr_` holder whose spelling names nothing the user
    /// wrote.
    pub(super) fn bound_names(&self, out: &mut Vec<String>) {
        match self {
            PatShape::Ary { elems, rest } => {
                for e in elems {
                    match e {
                        AryElem::Elide => {}
                        AryElem::Bind { name, .. } => out.push(name.clone()),
                        AryElem::Nested { pat, .. } => pat.bound_names(out),
                    }
                }
                match rest {
                    Some(RestShape::Bind(n)) => out.push(n.clone()),
                    Some(RestShape::Nested(pat)) => pat.bound_names(out),
                    None => {}
                }
            }
            PatShape::Obj { fields, rest } => {
                for f in fields {
                    match &f.binding {
                        ObjBinding::Bind { name, .. } => out.push(name.clone()),
                        ObjBinding::Nested { pat, .. } => pat.bound_names(out),
                    }
                }
                if let Some(n) = rest {
                    out.push(n.clone());
                }
            }
        }
    }
}

impl<'a> Parser<'a> {
    /// Recursive pattern reader — consumes from the opening `[` / `{`
    /// through its matching close. Err on a non-pattern shape; the
    /// for-of head scan save/restores around it, the statement
    /// drivers surface the message (they are committed after
    /// `let [` / `let {`).
    pub(super) fn read_pattern_shape(&mut self) -> Result<PatShape, String> {
        match self.peek() {
            Token::LBracket => self.read_ary_pattern(),
            Token::LBrace => self.read_obj_pattern(),
            t => Err(format!(
                "expected destructuring pattern, got {t:?} at {}",
                self.at()
            )),
        }
    }

    fn read_ary_pattern(&mut self) -> Result<PatShape, String> {
        self.pos += 1; // consume `[`
        let mut elems: Vec<AryElem> = Vec::new();
        let mut rest: Option<RestShape> = None;
        while !matches!(self.peek(), Token::RBracket) {
            if matches!(self.peek(), Token::Comma) {
                elems.push(AryElem::Elide);
                self.pos += 1;
                continue;
            }
            if matches!(self.peek(), Token::DotDotDot) {
                self.pos += 1;
                let bind = self.pattern_binding_name();
                let r = match self.peek() {
                    _ if bind.is_some() => {
                        self.pos += 1;
                        RestShape::Bind(bind.unwrap())
                    }
                    // §13.3.3 BindingRestElement : ... BindingPattern
                    // (`[...[x, y]]` / `[...{ len: length }]`).
                    Token::LBracket | Token::LBrace => {
                        RestShape::Nested(Box::new(self.read_pattern_shape()?))
                    }
                    t => {
                        return Err(format!(
                            "expected identifier or pattern after `...` in array destructuring, got {t:?} at {}",
                            self.at()
                        ));
                    }
                };
                rest = Some(r);
                break; // rest must be last — expect `]` next
            }
            let bind = self.pattern_binding_name();
            let elem = match self.peek() {
                _ if bind.is_some() => {
                    let name = bind.unwrap();
                    self.pos += 1;
                    let default = self.read_pattern_default()?;
                    AryElem::Bind { name, default }
                }
                Token::LBracket | Token::LBrace => {
                    let pat = self.read_pattern_shape()?;
                    let default = self.read_pattern_default()?;
                    AryElem::Nested { pat, default }
                }
                t => {
                    return Err(format!(
                        "expected identifier in array destructuring, got {t:?} at {}",
                        self.at()
                    ));
                }
            };
            elems.push(elem);
            if matches!(self.peek(), Token::Comma) {
                self.pos += 1;
            } else {
                break;
            }
        }
        match self.peek() {
            Token::RBracket => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `]` ending array destructuring, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        Ok(PatShape::Ary { elems, rest })
    }

    fn read_obj_pattern(&mut self) -> Result<PatShape, String> {
        self.pos += 1; // consume `{`
        let mut fields: Vec<ObjField> = Vec::new();
        let mut rest: Option<String> = None;
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::DotDotDot) {
                self.pos += 1;
                let bind = self.pattern_binding_name();
                let n = match self.peek() {
                    _ if bind.is_some() => bind.unwrap(),
                    t => {
                        return Err(format!(
                            "expected identifier after `...` in object destructuring, got {t:?} at {}",
                            self.at()
                        ));
                    }
                };
                self.pos += 1;
                rest = Some(n);
                break;
            }
            // §14.3.3 ComputedPropertyName — `{ [expr]: binding }`.
            // The key parses as a real expression and evaluates at
            // bind time; a computed key has no shorthand form, so the
            // `:` is mandatory (grammar, not a recorded boundary).
            if matches!(self.peek(), Token::LBracket) {
                self.pos += 1;
                let key_expr = self.with_yield_hoist_disallowed(|p| p.parse_assign())?;
                match self.peek() {
                    Token::RBracket => self.pos += 1,
                    t => {
                        return Err(format!(
                            "expected `]` after computed key in object destructuring, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
                match self.peek() {
                    Token::Colon => self.pos += 1,
                    t => {
                        return Err(format!(
                            "expected `:` after computed key in object destructuring, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
                let binding = self.read_obj_binding_target()?;
                fields.push(ObjField {
                    key: FieldKey::Computed(key_expr),
                    binding,
                });
                if matches!(self.peek(), Token::Comma) {
                    self.pos += 1;
                    continue;
                }
                break;
            }
            // Reserved-word tokens are valid field NAMES but need an
            // explicit `field: binding` rename (the binding itself
            // can't be a reserved word) — same rule as the object
            // literal wedge.
            // Asked before the match because admitting an `await`
            // records the site, which needs `&mut self` — a guard
            // cannot. The answer is only consulted below, after the
            // two arms that claim a token this one never sees.
            // Only ASK when the word can actually shorthand-bind.
            // `{ let: x }` names the FIELD `let` and binds `x`, so
            // admitting the word as a BINDING there — and parking it
            // for the goal gate — refused a module node runs. With a
            // `:` ahead the keyword arm below claims it as a property
            // name, which is what it is.
            let shorthand = if matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.token),
                Some(Token::Colon)
            ) {
                None
            } else {
                self.shorthand_binding_name()
            };
            let (field, field_is_kw) = match self.peek() {
                Token::Ident(n) => (PropKey::from(n), false),
                // A word that binds as an ordinary name shorthands
                // like any other name — no rename needed. Must precede
                // BOTH keyword arms below, which would otherwise claim
                // it and demand one. The escaped spelling reaches here
                // too: §12.7.2 judges a BindingIdentifier on its
                // StringValue, so `{ \u0061wait }` binds exactly where
                // `{ await }` does (r633 — it used to be sent to the
                // rename on the ground that "the BINDING cannot be
                // one", which was never true of the words this
                // position admits).
                _ if shorthand.is_some() => (PropKey::from(shorthand.unwrap()), false),
                // ES §12.7.2 — an escaped ReservedWord this position
                // does NOT admit still names the FIELD, and then the
                // rename is mandatory, exactly as for a bare keyword.
                Token::EscapedIdent(n) => (PropKey::from(n), true),
                t if Self::keyword_property_name(t).is_some() => {
                    (PropKey::from(Self::keyword_property_name(t).unwrap()), true)
                }
                // §13.3.3 PropertyName : NumericLiteral — `{ 0: v,
                // length: z }` (the array-as-object pattern family).
                // A numeric key cannot shorthand-bind, so it takes
                // the same mandatory-rename path as keyword fields; the
                // load recipe turns an all-digit field into an index
                // read. Non-integer numerics keep the loud reject.
                Token::Number(n) if n.fract() == 0.0 && *n >= 0.0 => {
                    (PropKey::from((*n as u64).to_string()), true)
                }
                // PropertyName : StringLiteral — `{ "a b": v }`; the
                // field is the cooked string, rename mandatory.
                Token::String(s) => (PropKey::from(s.clone()), true),
                t => {
                    return Err(format!(
                        "expected identifier in object destructuring, got {t:?} at {}",
                        self.at()
                    ));
                }
            };
            self.pos += 1;
            let binding = if matches!(self.peek(), Token::Colon) {
                self.pos += 1;
                self.read_obj_binding_target()?
            } else {
                if field_is_kw {
                    let field = field.lossy();
                    return Err(format!(
                        "destructuring field `{field}` is a reserved word; use `{{ {field}: <binding> }}` to rename at {}",
                        self.at()
                    ));
                }
                let default = self.read_pattern_default()?;
                // Shorthand binds the Ident arm's own spelling.
                ObjBinding::Bind {
                    name: field.to_string(),
                    default,
                }
            };
            fields.push(ObjField {
                key: FieldKey::Named(field),
                binding,
            });
            if matches!(self.peek(), Token::Comma) {
                self.pos += 1;
            } else {
                break;
            }
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` ending object destructuring, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        Ok(PatShape::Obj { fields, rest })
    }

    /// The binding half after a field's `:` — a fresh name (with an
    /// optional default) or a nested pattern. Shared by the static and
    /// computed key arms of [`Self::read_obj_pattern`].
    fn read_obj_binding_target(&mut self) -> Result<ObjBinding, String> {
        let bind = self.pattern_binding_name();
        match self.peek() {
            _ if bind.is_some() => {
                let name = bind.unwrap();
                self.pos += 1;
                let default = self.read_pattern_default()?;
                Ok(ObjBinding::Bind { name, default })
            }
            Token::LBracket | Token::LBrace => {
                let pat = self.read_pattern_shape()?;
                let default = self.read_pattern_default()?;
                Ok(ObjBinding::Nested { pat, default })
            }
            t => Err(format!(
                "expected rename target after `:` in destructuring, got {t:?} at {}",
                self.at()
            )),
        }
    }

    /// Optional `= <default>` after a binding / nested pattern slot.
    /// Defaults evaluate conditionally (only on undefined) — no
    /// yield hoist.
    fn read_pattern_default(&mut self) -> Result<Option<ExprId>, String> {
        if matches!(self.peek(), Token::Eq) {
            self.pos += 1;
            Ok(Some(
                self.with_yield_hoist_disallowed(|p| p.parse_assign())?,
            ))
        } else {
            Ok(None)
        }
    }

    /// Statement driver — `let/const PAT (: T)? = src ;` for both
    /// pattern kinds (parse_stmt dispatches here on `let [` /
    /// `let {`). Replaces the flat parse_array_destructuring /
    /// parse_object_destructuring pair.
    pub(super) fn parse_destructuring_decl(
        &mut self,
        mutable: bool,
        is_var: bool,
    ) -> Result<Stmt, String> {
        // §14.3.1.1 — `var { let } = o` binds, `let`/`const { let }` do
        // not. Restored before `?` so an error does not leak the flag.
        let saved = std::mem::replace(&mut self.pattern_binds_lexically, !is_var);
        let pat = self.read_pattern_shape();
        self.pattern_binds_lexically = saved;
        let pat = pat?;
        // Optional `: T` annotation on the whole pattern — the
        // desugared bindings get their types from the source via
        // inference, so the annotation is parsed and discarded.
        if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            let _ann = self.parse_type_ann()?;
        }
        match self.peek() {
            Token::Eq => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `=` after destructuring pattern, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let src = self.parse_expr()?;
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        let mut stmts: Vec<Stmt> = Vec::new();
        self.emit_pattern_binds(&pat, src, mutable, is_var, &mut stmts);
        // Stmt::Multi flattens at lowering time, so the user-visible
        // lets join the surrounding scope rather than a fresh frame.
        Ok(Stmt::Multi(stmts))
    }
}
