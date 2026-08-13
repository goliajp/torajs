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
//! Recorded boundaries (loud, never silent): computed / numeric /
//! string keys in object patterns; `for (let PAT in obj)` pattern
//! heads; CoverInitializedName in assignment-position object heads.

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
    pub(super) field: String,
    pub(super) binding: ObjBinding,
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
                let r = match self.peek() {
                    Token::Ident(n) => {
                        let n = n.clone();
                        self.pos += 1;
                        RestShape::Bind(n)
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
            let elem = match self.peek() {
                Token::Ident(n) => {
                    let name = n.clone();
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
                let n = match self.peek() {
                    Token::Ident(n) => n.clone(),
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
            // Reserved-word tokens are valid field NAMES but need an
            // explicit `field: binding` rename (the binding itself
            // can't be a reserved word) — same rule as the object
            // literal wedge.
            let (field, field_is_kw) = match self.peek() {
                Token::Ident(n) => (n.clone(), false),
                // ES §12.7.2 — an escaped ReservedWord names the
                // FIELD; like a bare keyword it still needs the
                // explicit rename, since the BINDING cannot be one.
                Token::EscapedIdent(n) => (n.clone(), true),
                t if Self::keyword_property_name(t).is_some() => {
                    (Self::keyword_property_name(t).unwrap().to_string(), true)
                }
                // §13.3.3 PropertyName : NumericLiteral — `{ 0: v,
                // length: z }` (the array-as-object pattern family).
                // A numeric key cannot shorthand-bind, so it takes
                // the same mandatory-rename path as keyword fields; the
                // load recipe turns an all-digit field into an index
                // read. Non-integer numerics keep the loud reject.
                Token::Number(n) if n.fract() == 0.0 && *n >= 0.0 => {
                    ((*n as u64).to_string(), true)
                }
                // PropertyName : StringLiteral — `{ "a b": v }`; the
                // field is the cooked string, rename mandatory.
                Token::String(s) => (s.clone(), true),
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
                match self.peek() {
                    Token::Ident(n) => {
                        let name = n.clone();
                        self.pos += 1;
                        let default = self.read_pattern_default()?;
                        ObjBinding::Bind { name, default }
                    }
                    Token::LBracket | Token::LBrace => {
                        let pat = self.read_pattern_shape()?;
                        let default = self.read_pattern_default()?;
                        ObjBinding::Nested { pat, default }
                    }
                    t => {
                        return Err(format!(
                            "expected rename target after `:` in destructuring, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
            } else {
                if field_is_kw {
                    return Err(format!(
                        "destructuring field `{field}` is a reserved word; use `{{ {field}: <binding> }}` to rename at {}",
                        self.at()
                    ));
                }
                let default = self.read_pattern_default()?;
                ObjBinding::Bind {
                    name: field.clone(),
                    default,
                }
            };
            fields.push(ObjField { field, binding });
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
        let pat = self.read_pattern_shape()?;
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
