//! Class-decl member helpers (static block / member name / untyped
//! field / field-init finalize), split from
//! `parse_class_decl.rs::parse_class_decl_with_abstract` (2026-07-03,
//! fn-debt decomp). Bodies verbatim; mechanical rewrites:
//! `name.clone()` → `name.to_string()` (`&str` param, same as the
//! chunk-174/175 method/field siblings), `explicit_visibility = ..`
//! → `*explicit_visibility = ..` (`&mut` param), tail returns
//! adjusted.

use super::*;

impl<'a> Parser<'a> {
    /// `static { stmts }` class static block (P8.3-A2, ES2022
    /// §15.7.10). Caller detected `static` + `{` and this consumes
    /// both plus the block body, pushing StaticInit::Block.
    pub(super) fn parse_class_static_block(
        &mut self,
        name: &str,
        static_init: &mut Vec<StaticInit>,
    ) -> Result<(), String> {
        self.pos += 2; // consume `static` + `{`
        // S2.9 — a static block is not a constructor, so `super()` in it
        // is an early SyntaxError (ES §15.7.1).
        let saved_super = std::mem::replace(&mut self.super_call_allowed, false);
        let mut block_stmts: Vec<Stmt> = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let s = self.parse_stmt()?;
            block_stmts.push(s);
        }
        self.super_call_allowed = saved_super;
        if !matches!(self.peek(), Token::RBrace) {
            return Err(format!(
                "expected `}}` to close static-block in class `{name}` at {}",
                self.at()
            ));
        }
        self.pos += 1; // consume `}`
        static_init.push(StaticInit::Block(block_stmts));
        Ok(())
    }

    /// Class member name: computed `[Symbol.x]` key (P5.2),
    /// `#name` PrivateIdentifier (P8.1, mangled + forced Private),
    /// reserved-word member names (V3-18), or plain ident. Returns
    /// `(member_name, consumed_computed_name)`.
    pub(super) fn parse_class_member_name(
        &mut self,
        name: &str,
        _is_static: bool,
        explicit_visibility: &mut Option<ast::Visibility>,
    ) -> Result<(String, bool), String> {
        // P5.2 — computed-key class member `[Symbol.iterator]() {
        // ... }`. Mirrors the object-literal computed-key handling
        // (parse_object_field) so the same `__sym_Symbol_iterator__`
        // synthetic name flows through into the class layout. Only
        // member-name shape `[<Ident>(. <Ident>)*]` is accepted —
        // string-literal keys (`["foo"]`) and arbitrary exprs are
        // out of scope for the class-method computed-key surface.
        // Body parsing falls through to the normal method branch
        // by emitting a synthetic name + advancing past `]`.
        let mut consumed_computed_name = false;
        let member_name = if matches!(self.peek(), Token::LBracket) {
            self.pos += 1;
            let key = match self.peek() {
                Token::Ident(_) => {
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
                Token::String(s) => {
                    let k = s.clone();
                    self.pos += 1;
                    k
                }
                t => {
                    return Err(format!(
                        "expected `Symbol.iterator`-style key inside `[...]` for class `{name}` member, got {t:?} at {}",
                        self.at()
                    ));
                }
            };
            if !matches!(self.peek(), Token::RBracket) {
                let t = self.peek().clone();
                return Err(format!(
                    "expected `]` to close computed class member key for `{name}`, got {t:?} at {}",
                    self.at()
                ));
            }
            self.pos += 1; // consume `]`
            consumed_computed_name = true;
            key
        } else {
            match self.peek() {
                Token::Ident(n) => n.clone(),
                // P8.1 — `#name` PrivateIdentifier as a class member
                // name. Two effects: (a) mangle the name to
                // `__priv_<ClassName>__<name>` so the existing
                // public-field machinery (struct_layouts, member
                // resolution, codegen) handles it uniformly without
                // a parallel data path; (b) force visibility to
                // Private regardless of any earlier `public`/
                // `protected` modifier — `#` is the spec marker
                // for hard-private (exact-class-only access),
                // distinct from the TS-modifier `private` which
                // also allows Protected-style subclass access.
                // Cross-class enforcement happens at typecheck
                // (check.rs P8.1-A4); ssa_lower sees a regular
                // String name (P8.1-A5 validates round-trip).
                //
                // `static #x` (P-SURF S2.37) mangles exactly like the
                // instance path — the static machinery keys off the
                // member NAME (`__sf_<C>__<mangled>` / `__sm_<C>__
                // <mangled>`), so a pre-mangled name flows through it
                // unchanged. Access sites (`C.#x` / `this.#x` in a
                // static body) mangle at `member_name_after_dot` to
                // the same `__priv_<C>__<n>`, matching the rewrite
                // table keys Pass 2.5 builds from this declaration.
                Token::PrivateIdent(n) => {
                    let priv_name = n.clone();
                    *explicit_visibility = Some(ast::Visibility::Private);
                    format!("__priv_{name}__{priv_name}")
                }
                // V3-18 wedge — accept the full reserved-word list
                // as class member names per ES spec §12.7.6
                // (PropertyName allows IdentifierName which includes
                // reserved words). Routed through the centralized
                // keyword_property_name helper so all four
                // property-name positions stay in sync.
                t if Self::keyword_property_name(t).is_some() => {
                    Self::keyword_property_name(t).unwrap().to_string()
                }
                t => {
                    return Err(format!(
                        "expected class member name, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        };
        Ok((member_name, consumed_computed_name))
    }

    /// Untyped class field with initializer (`name = init` /
    /// `static name = init`, V3-18) — type inferred from a literal
    /// initializer shape. Sibling of the chunk-175 typed-field
    /// split.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn parse_class_member_field_untyped(
        &mut self,
        name: &str,
        member_name: String,
        consumed_computed_name: bool,
        explicit_visibility: Option<ast::Visibility>,
        is_readonly: bool,
        is_abstract_method: bool,
        is_static: bool,
        fields: &mut Vec<(String, String)>,
        static_init: &mut Vec<StaticInit>,
        field_inits: &mut Vec<(String, ExprId)>,
    ) -> Result<(), String> {
        // V3-18 wedge — class field with no explicit type
        // ann (`name = init` / `static name = init`). Per
        // TS spec the type is inferred from the init
        // expression; subset infers from literal-shape
        // (Number / String / Boolean / Array of literal /
        // ObjectLit). Other init shapes fall back to
        // requiring an explicit ann.
        if is_abstract_method {
            return Err(format!(
                "`abstract` modifier is only valid on methods, not on field `{member_name}` in class `{name}` at {}",
                self.at()
            ));
        }
        if consumed_computed_name {
            self.pos += 1; // consume `=` only
        } else {
            self.pos += 2; // consume name + `=`
        }
        let init = self.parse_assign()?;
        let inferred = match self.ast.get_expr(init) {
            Expr::Number(_) => "number",
            Expr::String(_) => "string",
            Expr::Bool(_) => "boolean",
            _ => {
                return Err(format!(
                    "untyped class field `{member_name}` requires a literal initializer (number / string / boolean) for type inference at {}",
                    self.at()
                ));
            }
        };
        let ty = inferred.to_string();
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        let visibility = explicit_visibility.unwrap_or(ast::Visibility::Public);
        if visibility != ast::Visibility::Public {
            self.ast
                .member_visibility
                .insert((name.to_string(), member_name.clone()), visibility);
        }
        if is_readonly {
            self.ast
                .readonly_fields
                .insert((name.to_string(), member_name.clone()));
        }
        if is_static {
            static_init.push(StaticInit::Field(ast::StaticField {
                name: member_name,
                type_ann: ty,
                init,
            }));
        } else {
            field_inits.push((member_name.clone(), init));
            fields.push((member_name, ty));
        }
        Ok(())
    }

    /// Prepend `this.<n> = <init>` stmts for each collected field
    /// initializer (V3-18), synthesizing an empty ctor if none was
    /// declared so the inits still run on `new C(...)`.
    pub(super) fn finalize_class_field_inits(
        &mut self,
        class_name: &str,
        field_inits: Vec<(String, ExprId)>,
        ctor: Option<ClassCtor>,
    ) -> Option<ClassCtor> {
        if field_inits.is_empty() {
            return ctor;
        }
        let mut prefix: Vec<Stmt> = Vec::new();
        for (fname, init_expr) in &field_inits {
            let this_ref = self.ast.add_expr(Expr::This);
            let lhs = self.ast.add_expr(Expr::Member {
                obj: this_ref,
                name: fname.clone(),
            });
            let assign = self.ast.add_expr(Expr::Assign {
                target: lhs,
                value: *init_expr,
            });
            prefix.push(Stmt::Expr(assign));
        }
        Some(match ctor {
            Some(c) => {
                // §10.2.11 — the initializers run once `this` exists,
                // so in a derived class they belong AFTER the
                // `super(...)`, not at the top. In front of it, an
                // initializer reading an inherited field saw the slot
                // before the base ctor filled it (`e = this.v + 1`
                // answered 1 against a base `v = 5`). No super call in
                // the body means either a base class or a ctor that
                // never initializes `this` at all — the top is right
                // for the first and moot for the second.
                let at = self.super_stmt_index(&c.body).map_or(0, |i| i + 1);
                let mut body = c.body;
                body.splice(at..at, prefix);
                ClassCtor {
                    params: c.params,
                    body,
                }
            }
            None => {
                // The class declared no ctor; this one exists only to
                // hold the initializers. Record that, so the derived
                // default-ctor synthesis still treats the class as
                // having the implicit ctor it actually has.
                self.ast
                    .field_init_synth_ctors
                    .insert(class_name.to_string());
                ClassCtor {
                    params: Vec::new(),
                    body: prefix,
                }
            }
        })
    }

    /// Index of the first statement in `body` that IS a `super(...)`
    /// call. Only the statement level is examined: a super buried in a
    /// branch has no single "after" to speak of, and that shape is a
    /// recorded boundary elsewhere too.
    fn super_stmt_index(&self, body: &[Stmt]) -> Option<usize> {
        body.iter().position(|s| {
            matches!(s, Stmt::Expr(eid) if matches!(self.ast.get_expr(*eid), Expr::Super { .. }))
        })
    }
}
