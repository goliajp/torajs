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
        // is an early SyntaxError (ES §15.7.1). §15.7.1 also forbids
        // `await` anywhere in a ClassStaticBlockBody.
        let saved_super = std::mem::replace(&mut self.super_call_allowed, false);
        let saved_await = std::mem::replace(&mut self.await_allowed, false);
        let mut block_stmts: Vec<Stmt> = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let s = match self.parse_stmt() {
                Ok(s) => s,
                Err(e) => {
                    self.await_allowed = saved_await;
                    self.super_call_allowed = saved_super;
                    return Err(e);
                }
            };
            block_stmts.push(s);
        }
        self.await_allowed = saved_await;
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
        // P5.2 + RFC 20260802-class-computed-member 刀 1 — computed
        // class member key `[<key>]`. Three compile-time-foldable
        // shapes install under a static name:
        //   * `[Symbol.<chain>]` → `__sym_Symbol_<chain>__` (the
        //     vtable / iterator-protocol consumers key off exactly
        //     this synthetic name),
        //   * `["str"]` whole-literal key → the string itself,
        //   * `[0x10]` whole-literal numeric key → its §6.1.6.1.20
        //     canonical string ("16"), same fold as object literals.
        // Every other expression is a RUNTIME computed key
        // (§15.4 ClassElementName → ComputedPropertyName evaluates at
        // class-definition time); the runtime install lane is 刀 2 of
        // the RFC, so those reject loudly here. The old behaviour —
        // folding ANY ident chain to `__sym_<chain>__` — silently
        // installed `[k]()` under a name unrelated to the runtime
        // value of `k`, which is exactly the silent-wrong shape the
        // reject replaces.
        // Body parsing falls through to the normal method branch
        // by emitting a synthetic name + advancing past `]`.
        let mut consumed_computed_name = false;
        let member_name = if matches!(self.peek(), Token::LBracket) {
            self.pos += 1;
            let key = match self.peek() {
                Token::Ident(head)
                    if head == "Symbol"
                        && matches!(
                            self.tokens.get(self.pos + 1).map(|t| &t.token),
                            Some(Token::Dot)
                        ) =>
                {
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
                Token::String(s)
                    if matches!(
                        self.tokens.get(self.pos + 1).map(|t| &t.token),
                        Some(Token::RBracket)
                    ) =>
                {
                    let k = s.clone();
                    self.pos += 1;
                    k
                }
                Token::Number(n)
                    if matches!(
                        self.tokens.get(self.pos + 1).map(|t| &t.token),
                        Some(Token::RBracket)
                    ) =>
                {
                    let k = crate::ast::number_prop_key(*n);
                    self.pos += 1;
                    k
                }
                _ => {
                    // RFC 20260802 刀 2 — every other expression is a
                    // RUNTIME computed key (§15.4 ClassElementName →
                    // ComputedPropertyName evaluates at class-
                    // definition time). The member installs under a
                    // unique `__ccm_<n>__` sentinel through the
                    // ordinary method / accessor machinery;
                    // desugar_classes emits the ToPropertyKey +
                    // define patch at the class-decl position.
                    let key_expr = self.parse_assign()?;
                    let sentinel = format!("__ccm_{}__", self.ast.class_computed_keys.len());
                    self.ast
                        .class_computed_keys
                        .insert((name.to_string(), sentinel.clone()), key_expr);
                    sentinel
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
                    // ES §15.7.1 early error — `ClassElementName :
                    // PrivateIdentifier` is a Syntax Error if its
                    // StringValue is "#constructor", in EVERY member
                    // position (field, method, accessor, generator).
                    if n == "constructor" {
                        return Err(format!(
                            "class member may not be named `#constructor` in class `{name}` at {} (ES §15.7.1)",
                            self.at()
                        ));
                    }
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
                // ES §12.7.2 — the same IdentifierName written with an
                // escape (`break() {}`).
                Token::EscapedIdent(n) => n.clone(),
                // RFC 20260802-class-computed-member 刀 1 — §12.7.6
                // PropertyName literal forms as direct member names:
                // `'default'() {}` / `get 'str'()` fold to the string
                // itself; `0x10() {}` / `get 1.0()` fold to the
                // §6.1.6.1.20 canonical string ("16" / "1"), the same
                // fold object literals use. Single tokens, so the
                // shared "name still unconsumed" protocol holds.
                Token::String(s) => s.clone(),
                Token::Number(n) => crate::ast::number_prop_key(*n),
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

    /// Field-declaration dispatch — the `:` / `=` / `;` / `}` arms
    /// of the member loop share one 10-parameter surface and differ
    /// only in which field parser runs (rotation 240: collapsed out
    /// of `parse_class_decl_with_abstract` when the S2.40 empty-
    /// element skip pushed it past the 200-line fn limit).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn parse_class_member_field_dispatch(
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
        // RFC 20260802 刀 3 后半 — runtime computed FIELD; tail parse
        // in the `parse_class_decl_computed_field.rs` sibling.
        if member_name.starts_with("__ccm_") {
            return self.parse_computed_field_tail(name, member_name, is_static, field_inits);
        }
        // ES §15.7.1 early error — `ClassElement : FieldDefinition;`
        // is a Syntax Error if PropName of FieldDefinition is
        // "constructor" (ident, string-literal, or folded `['...']`
        // spelling all produce that PropName). A COMPUTED
        // `["constructor"]` key is legal — PropName of a
        // ComputedPropertyName is empty — and the literal fold above
        // already flipped `consumed_computed_name` for it, so that
        // flag is exactly the spec's LiteralPropertyName test. The
        // method position stays untouched: `'constructor'() {}` IS
        // the constructor and never reaches this field dispatch.
        if !consumed_computed_name && member_name == "constructor" {
            return Err(format!(
                "class field may not be named `constructor` in class `{name}` at {} (ES §15.7.1)",
                self.at()
            ));
        }
        let field_inits_mark = field_inits.len();
        let static_init_mark = static_init.len();
        // Same lookahead the member loop matched on: the member name
        // is still unconsumed unless the computed-name path already
        // ate it (`name + ]`), so the shape token sits one ahead.
        let idx = if consumed_computed_name {
            self.pos
        } else {
            self.pos + 1
        };
        match self.tokens.get(idx).map(|t| &t.token) {
            Some(Token::Colon) => self.parse_class_member_field_typed(
                name,
                member_name,
                consumed_computed_name,
                explicit_visibility,
                is_readonly,
                is_abstract_method,
                is_static,
                fields,
                static_init,
                field_inits,
            )?,
            Some(Token::Eq) => self.parse_class_member_field_untyped(
                name,
                member_name,
                consumed_computed_name,
                explicit_visibility,
                is_readonly,
                is_abstract_method,
                is_static,
                fields,
                static_init,
                field_inits,
            )?,
            _ => self.parse_class_member_field_bare(
                name,
                member_name,
                consumed_computed_name,
                explicit_visibility,
                is_readonly,
                is_abstract_method,
                is_static,
                fields,
                static_init,
                field_inits,
            )?,
        }
        // ES §15.7.1 early error — it is a Syntax Error if
        // ContainsArguments of the field's Initializer is true. The
        // three field parsers above push at most one entry onto
        // `field_inits` (instance) or `static_init` (static), so the
        // pre-call marks identify exactly this member's initializer.
        for (_, init) in &field_inits[field_inits_mark..] {
            if super::class_field_early_errors::init_contains_arguments(&self.ast, *init) {
                return Err(format!(
                    "`arguments` is not allowed in a class field initializer in class `{name}` at {} (ES §15.7.1)",
                    self.at()
                ));
            }
        }
        for si in &static_init[static_init_mark..] {
            if let StaticInit::Field(f) = si
                && super::class_field_early_errors::init_contains_arguments(&self.ast, f.init)
            {
                return Err(format!(
                    "`arguments` is not allowed in a class field initializer in class `{name}` at {} (ES §15.7.1)",
                    self.at()
                ));
            }
        }
        Ok(())
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
            // Anything else takes the `any` slot rather than rejecting
            // the program. Real inference needs the initializer's TYPE,
            // which only the checker has — the parser can read shape and
            // nothing more, so scalar literals are the whole of what it
            // can honestly narrow. Refusing the rest bought no safety
            // (the initializer is still parsed and checked; only the
            // SLOT widens) while costing every field whose init is a
            // call, an array, an object, or null. Narrowing these
            // properly belongs in the checker and is registered; doing
            // it here would mean re-implementing type inference on
            // syntax.
            _ => "any",
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
            // A `__ccm_<n>__` sentinel field becomes a KEYED write
            // into the expando dict (lhs built in the sibling).
            let lhs = if fname.starts_with("__ccm_") {
                self.computed_field_init_lhs(class_name, fname, this_ref)
            } else {
                self.ast.add_expr(Expr::Member {
                    obj: this_ref,
                    name: fname.clone(),
                })
            };
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
