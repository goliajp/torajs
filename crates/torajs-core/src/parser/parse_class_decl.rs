//! `Parser::parse_class_decl_with_abstract` extracted from
//! `parser.rs` (chunk 165). Largest single parser god-fn.
//!
//! Pre-extract this method was 638 LOC inside `impl Parser` block.
//! Body verbatim moves here as impl-block sibling (same pattern as
//! chunks 162/163/164's parse_stmt / try_parse_for_of /
//! parse_postfix extractions).
//!
//! `parse_class_decl_with_abstract` parses `[abstract] class C
//! [extends P]<T...> { fields, ctor, methods, static_methods }`
//! including abstract modifier, generic params, extends parent,
//! field decls, constructor (special-cased), instance methods,
//! static methods + static field initializers (StaticInit::Block
//! for static blocks), accessor getters/setters (P8.2 — separate
//! AccessorKind).
//!
//! Body unchanged.

use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_class_decl_with_abstract(
        &mut self,
        is_abstract: bool,
        allow_anon: bool,
        force_synth: bool,
    ) -> Result<Stmt, String> {
        self.pos += 1; // consume `class`
        let name = match self.peek() {
            // P8.5 — `force_synth`: even when a class-expression-position
            // class carries an inner name (`class Inner { ... }`),
            // consume-and-discard it so the synth name controls all
            // downstream resolution. Inner self-binding (Inner referring
            // to the class inside its own body) is an L3b follow-up.
            Token::Ident(_) if force_synth => {
                self.pos += 1;
                let id = self.mint_desugar_id();
                format!("__ClassExpr_{id}")
            }
            Token::Ident(n) => {
                let n = n.clone();
                self.pos += 1;
                n
            }
            // P8.5 — anonymous class expression (`const F = class { ... }`,
            // `new (class { ... })()`). Mint a unique synth name that
            // `synthesize_class_globals` will expose as
            // `__class___ClassExpr_<id>`. Source-side anonymous → user
            // code never references this name directly; only the
            // parser-emitted Ident at the original use site does.
            _ if allow_anon => {
                let id = self.mint_desugar_id();
                format!("__ClassExpr_{id}")
            }
            t => {
                return Err(format!("expected class name, got {t:?} at {}", self.at()));
            }
        };
        // P8.1 — set `current_class` so private-member parsing inside
        // this class body can mangle `this.#x` to `__priv_<class>__x`.
        // Saved/restored at every successful return path; on `return
        // Err(...)` we don't restore (parse has failed; parser state
        // is moot). Cloned for nested classes — the recursive call
        // overwrites; we restore the outer name on its successful exit.
        let saved_class = self.current_class.take();
        self.current_class = Some(name.clone());
        // Optional generic type params: `class Map<K, V> { ... }`.
        let mut type_params: Vec<String> = Vec::new();
        if matches!(self.peek(), Token::Lt) {
            self.pos += 1;
            if !matches!(self.peek(), Token::Gt) {
                loop {
                    match self.peek() {
                        Token::Ident(n) => {
                            type_params.push(n.clone());
                            self.pos += 1;
                        }
                        t => {
                            return Err(format!(
                                "expected type-param name in class<...>, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    match self.peek() {
                        Token::Comma => self.pos += 1,
                        Token::Gt => break,
                        t => {
                            return Err(format!(
                                "expected `,` or `>` in class type params, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                }
            }
            match self.peek() {
                Token::Gt => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `>` to close class type params, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        }
        // M5.2 — optional `extends BaseName` clause.
        let parent: Option<String> = if matches!(self.peek(), Token::Extends) {
            self.pos += 1;
            match self.peek() {
                Token::Ident(n) => {
                    let n = n.clone();
                    self.pos += 1;
                    Some(n)
                }
                t => {
                    return Err(format!(
                        "expected parent class name after `extends`, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        } else {
            None
        };
        // V3-18 wedge — `implements Foo, Bar` clause on class.
        // Per TS spec §3.7, `implements` declares structural-typing
        // intent without runtime effect. Subset consumes and
        // discards the list — the structural check is provided by
        // existing field-by-field typecheck on assignment.
        if let Token::Ident(s) = self.peek()
            && s == "implements"
        {
            self.pos += 1;
            loop {
                let _iface = self.parse_type_ann()?;
                if matches!(self.peek(), Token::Comma) {
                    self.pos += 1;
                    continue;
                }
                break;
            }
        }
        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` to begin class body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let mut fields: Vec<(String, String)> = Vec::new();
        let mut static_init: Vec<StaticInit> = Vec::new();
        let mut ctor: Option<ClassCtor> = None;
        let mut methods: Vec<ClassMethod> = Vec::new();
        let mut static_methods: Vec<ClassMethod> = Vec::new();
        // V3-18 wedge — instance-field initializers (`val: T = init`).
        // Collected here in source order; appended to the ctor body
        // (a synthesized one if no ctor was declared) at class-decl
        // finalization. The synthesized prefix is "this.<n> = init"
        // per declared field.
        let mut field_inits: Vec<(String, ExprId)> = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            // Each member is one of:
            //   - `constructor(params) { body }`
            //   - `methodName(params): R? { body }`
            //   - `fieldName: T;`                       (instance field)
            //   - `static methodName(params): R? { body }`  (M-OO.4)
            //   - `static fieldName: T = init;`              (M-OO.4)
            //   - `static { stmts; }`                        (P8.3-A2; ES2022 §15.7.10)
            // We disambiguate by lookahead: ident then `(` ⇒ ctor or method;
            // ident then `:` ⇒ field declaration. The `static` modifier is a
            // contextual keyword: only treated as such when the next token
            // is a valid member name shape.

            // P8.3-A2 — `static { ... }` class static block (ES2022 §15.7.10).
            // Detected at the top of each iteration, before modifier parsing,
            // because the `is_static`-modifier dispatch below assumes `static`
            // precedes a member NAME, not a block body. Visibility / readonly
            // / abstract are not valid on static blocks per spec, so refusing
            // to consume them above the block is correct — `public static {}`
            // and similar fall through to the existing modifier-misapplication
            // error.
            if let Token::Ident(s) = self.peek()
                && s == "static"
                && matches!(
                    self.tokens.get(self.pos + 1).map(|t| &t.token),
                    Some(Token::LBrace)
                )
            {
                self.pos += 2; // consume `static` + `{`
                let mut block_stmts: Vec<Stmt> = Vec::new();
                while !matches!(self.peek(), Token::RBrace | Token::Eof) {
                    let s = self.parse_stmt()?;
                    block_stmts.push(s);
                }
                if !matches!(self.peek(), Token::RBrace) {
                    return Err(format!(
                        "expected `}}` to close static-block in class `{name}` at {}",
                        self.at()
                    ));
                }
                self.pos += 1; // consume `}`
                static_init.push(StaticInit::Block(block_stmts));
                continue;
            }

            // Modifier prefix — see `parser/class_member.rs`.
            let ClassMemberModifierPrefix {
                mut explicit_visibility,
                is_readonly,
                is_abstract_method,
                is_static,
                accessor_kind,
                is_async,
            } = self.parse_class_member_modifier_prefix(&name, is_abstract)?;
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
                    // `static #x` is out of P8.1 scope — reject here
                    // with a targeted error rather than silently
                    // synthesizing a static mangled name we can't yet
                    // lower correctly. The `is_static` lookahead above
                    // recognizes `static <PrivateIdent>` so we land in
                    // this arm.
                    Token::PrivateIdent(n) => {
                        if is_static {
                            return Err(format!(
                                "static private fields (`static #{n}`) not yet supported in class `{name}` — defer P8.x followup (at {})",
                                self.at()
                            ));
                        }
                        let priv_name = n.clone();
                        explicit_visibility = Some(ast::Visibility::Private);
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
            let next_tok = if consumed_computed_name {
                // We already consumed name + `]`, so the next token is
                // the one driving the member-shape decision (LParen
                // for method, Colon for field, Eq for typed-field).
                self.tokens.get(self.pos).map(|s| &s.token)
            } else {
                self.tokens.get(self.pos + 1).map(|s| &s.token)
            };
            match next_tok {
                Some(Token::LParen) => {
                    // ctor or method
                    if !consumed_computed_name {
                        self.pos += 1; // consume name
                    }
                    let is_ctor_branch = member_name == "constructor";
                    let (params, promoted_props, destr_lets) = if is_ctor_branch {
                        let (p, pr, dl) = self.parse_ctor_param_list()?;
                        (p, pr, dl)
                    } else {
                        let (p, dl) = self.parse_param_list()?;
                        (p, Vec::new(), dl)
                    };
                    let return_type = if matches!(self.peek(), Token::Colon) {
                        self.pos += 1;
                        Some(self.parse_type_ann()?)
                    } else {
                        None
                    };
                    // V3-18 wedge — TS class-method overload signature:
                    // `methodName(...): R;`. Type-only, terminated by `;`.
                    // Skip and continue parsing the class body — the
                    // real impl is the trailing same-named decl.
                    if !is_abstract_method && matches!(self.peek(), Token::Semi) {
                        self.pos += 1;
                        continue;
                    }
                    let body = if is_abstract_method {
                        // M-OO.6 — abstract method has no body. ASI per
                        // ES spec: `;` is optional when the next token
                        // would naturally start a new statement, so
                        // accept the next class member directly. Common
                        // shape: `abstract area(): number\n  describe()`.
                        if matches!(self.peek(), Token::Semi) {
                            self.pos += 1;
                        }
                        Vec::new()
                    } else {
                        match self.peek() {
                            Token::LBrace => self.pos += 1,
                            t => {
                                return Err(format!(
                                    "expected `{{` for {member_name} body, got {t:?} at {}",
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
                                    "expected `}}` to end {member_name} body, got {t:?} at {}",
                                    self.at()
                                ));
                            }
                        }
                        // V3-18 wedge — prepend destr-param lets when
                        // class methods used a binding pattern.
                        if destr_lets.is_empty() {
                            body
                        } else {
                            let mut full = destr_lets;
                            full.extend(body);
                            full
                        }
                    };
                    if member_name == "constructor" {
                        if is_static {
                            return Err(format!(
                                "`static constructor` is not allowed in class `{name}`"
                            ));
                        }
                        if is_abstract_method {
                            return Err(format!(
                                "`abstract constructor` is not allowed in class `{name}`"
                            ));
                        }
                        if ctor.is_some() {
                            return Err(format!("duplicate constructor in class `{name}`"));
                        }
                        // V3-18 wedge — for each TS parameter-property
                        // (e.g. `public x: number`), promote to an
                        // instance field on the class and prepend
                        // `this.<n> = <n>` to the ctor body.
                        let mut body = body;
                        if !promoted_props.is_empty() {
                            let mut prefix: Vec<Stmt> = Vec::new();
                            for (idx, vis, rd) in &promoted_props {
                                let p = &params[*idx];
                                let ty_ann = p.type_ann.clone().unwrap_or_else(|| "any".into());
                                fields.push((p.name.clone(), ty_ann));
                                if *vis != ast::Visibility::Public {
                                    self.ast
                                        .member_visibility
                                        .insert((name.clone(), p.name.clone()), *vis);
                                }
                                if *rd {
                                    self.ast
                                        .readonly_fields
                                        .insert((name.clone(), p.name.clone()));
                                }
                                let this_ref = self.ast.add_expr(Expr::This);
                                let lhs = self.ast.add_expr(Expr::Member {
                                    obj: this_ref,
                                    name: p.name.clone(),
                                });
                                let rhs = self.ast.add_expr(Expr::Ident(p.name.clone()));
                                let assign = self.ast.add_expr(Expr::Assign {
                                    target: lhs,
                                    value: rhs,
                                });
                                prefix.push(Stmt::Expr(assign));
                            }
                            prefix.extend(body);
                            body = prefix;
                        }
                        ctor = Some(ClassCtor { params, body });
                    } else {
                        self.finalize_class_method(
                            &name,
                            member_name,
                            params,
                            return_type,
                            body,
                            explicit_visibility,
                            accessor_kind,
                            is_readonly,
                            is_abstract_method,
                            is_static,
                            is_async,
                            &mut methods,
                            &mut static_methods,
                        )?;
                    }
                }
                Some(Token::Colon) => {
                    // field declaration. Instance: `name: T;`. Static
                    // (M-OO.4): `name: T = init;` — init is required
                    // (no constructor to default-init in).
                    if is_abstract_method {
                        return Err(format!(
                            "`abstract` modifier is only valid on methods, not on field `{member_name}` in class `{name}` at {}",
                            self.at()
                        ));
                    }
                    if consumed_computed_name {
                        self.pos += 1; // consume colon only
                    } else {
                        self.pos += 2; // consume name + colon
                    }
                    let ty = self.parse_type_ann()?;
                    let visibility = explicit_visibility.unwrap_or(ast::Visibility::Public);
                    if visibility != ast::Visibility::Public {
                        self.ast
                            .member_visibility
                            .insert((name.clone(), member_name.clone()), visibility);
                    }
                    if is_readonly {
                        self.ast
                            .readonly_fields
                            .insert((name.clone(), member_name.clone()));
                    }
                    if is_static {
                        match self.peek() {
                            Token::Eq => self.pos += 1,
                            t => {
                                return Err(format!(
                                    "static field `{member_name}` requires an initializer (`= ...`), got {t:?} at {}",
                                    self.at()
                                ));
                            }
                        }
                        let init = self.parse_assign()?;
                        if matches!(self.peek(), Token::Semi) {
                            self.pos += 1;
                        }
                        static_init.push(StaticInit::Field(ast::StaticField {
                            name: member_name,
                            type_ann: ty,
                            init,
                        }));
                    } else {
                        // V3-18 wedge — accept `name: T = <init>` for
                        // instance fields. Init runs in ctor scope
                        // before user ctor body executes.
                        let init = if matches!(self.peek(), Token::Eq) {
                            self.pos += 1;
                            Some(self.parse_assign()?)
                        } else {
                            None
                        };
                        if matches!(self.peek(), Token::Semi) {
                            self.pos += 1;
                        }
                        if let Some(init_expr) = init {
                            field_inits.push((member_name.clone(), init_expr));
                        }
                        fields.push((member_name, ty));
                    }
                }
                Some(Token::Eq) => {
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
                            .insert((name.clone(), member_name.clone()), visibility);
                    }
                    if is_readonly {
                        self.ast
                            .readonly_fields
                            .insert((name.clone(), member_name.clone()));
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
                }
                t => {
                    return Err(format!(
                        "expected `(` (method) or `:` (field) after `{member_name}`, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` to end class body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        // V3-18 wedge — prepend `this.<n> = <init>` stmts for each
        // collected field initializer. Synthesize an empty ctor if
        // one wasn't declared so the inits still run on `new C(...)`.
        if !field_inits.is_empty() {
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
            ctor = Some(match ctor {
                Some(c) => {
                    let mut body = prefix;
                    body.extend(c.body);
                    ClassCtor {
                        params: c.params,
                        body,
                    }
                }
                None => ClassCtor {
                    params: Vec::new(),
                    body: prefix,
                },
            });
        }
        // P8.1 — restore the outer class context (parse-error paths
        // skip this; the parser is in an error state and the value
        // is moot).
        self.current_class = saved_class;
        Ok(Stmt::ClassDecl {
            name,
            type_params,
            parent,
            is_abstract,
            fields,
            static_init,
            ctor,
            methods,
            static_methods,
        })
    }
}
