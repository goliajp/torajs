//! `super` / `new` expression arms (chunk 421).
//!
//! Extracted verbatim (dedented one level) from parse_primary
//! (parser.rs) — the two largest match arms:
//! - parse_primary_super — `super(args)` ctor call, `super.m(args)`
//!   parent-method call (encoded as `__supercall__<m>` marker ident)
//! - parse_primary_new — `new C(args)` incl. `new.target`,
//!   `new class {...}()`, `new (expr)()`, type-arg skip, and the
//!   no-parens `new Foo` form
//!
//! Both assume the cursor is at the `super` / `new` token; the match
//! arms in parse_primary delegate here. Bodies unchanged.

use super::*;

/// What `new` is going to construct, as far as the parser can tell.
///
/// `Named` is the case the whole desugar chain is built around: the
/// target is spelled out, so a `__new_<C>` factory can be bound to it
/// at compile time. `Dynamic` is everything else — the target is an
/// expression and only the running program knows what it is.
enum NewHead {
    Named(String),
    Dynamic(ExprId),
}

impl<'a> Parser<'a> {
    pub(super) fn parse_primary_super(&mut self) -> Result<ExprId, String> {
        // `super(args)` — only valid inside a subclass ctor; the
        // desugar pass enforces that and rewrites to a Call to
        // `__cm_<Parent>__ctor(__this, args)`.
        // V3-18 wedge — `super.<method>(args)` (explicit
        // parent-method call): encoded as a Call to a marker
        // ident `__supercall__<methodname>`; desugar_classes
        // rewrites it to `__cm_<Parent>__<m>(__this, args)`
        // using the surrounding class's parent.
        self.pos += 1;
        // r334 blade 6 — SuperProperty position gate (§15.4.1 /
        // §15.7.1: `super` is an early SyntaxError outside a method /
        // field-init / static-block body). The gate covers all three
        // member spellings — `super.x`, `super[k]`, `super.m(...)` —
        // and matters twice over: bare illegal `super` fails at parse
        // phase (what the early-error negatives assert), and eval text
        // parsed WITHOUT the call site's home context fails the same
        // way, which the eval desugar converts into the runtime
        // SyntaxError §19.2.1.1 step 12 wants (`eval('super.x')` in
        // global code throws; nothing in the source runs).
        if matches!(self.peek(), Token::LBracket | Token::Dot) && !self.super_prop_allowed {
            return Err(format!(
                "`super` property access is only valid in a method, \
                 field initializer, or static block body, at {}",
                self.at()
            ));
        }
        // SuperProperty element form `super[expr]` (§13.3.5): encoded
        // as an Index off the `__superbase__` marker ident; the class
        // desugar rewrites the marker to the super base (the parent's
        // prototype object for instance members, the parent class
        // object for statics). A marker that survives to the checker
        // (super outside a class member) fails loud as an unknown
        // identifier.
        if matches!(self.peek(), Token::LBracket) {
            self.pos += 1;
            let index = self.parse_expr()?;
            match self.peek() {
                Token::RBracket => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `]` after `super[`, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            let marker = self.ast.add_expr(Expr::Ident("__superbase__".to_string()));
            return Ok(self.ast.add_expr(Expr::Index { obj: marker, index }));
        }
        if matches!(self.peek(), Token::Dot) {
            self.pos += 1;
            // §15.4.1 grammar — `super.#x` is never valid, in any
            // context: SuperProperty has no PrivateIdentifier
            // production (a parent's private names are lexically
            // invisible to the subclass anyway). Checked before
            // `expect_member_name`, which would otherwise mangle the
            // private name inside a class body and lose the reject.
            if let Token::PrivateIdent(n) = self.peek() {
                return Err(format!(
                    "`super.#{n}` — private names cannot be accessed \
                     through `super`, at {}",
                    self.at()
                ));
            }
            let m_name = self.expect_member_name("super.")?;
            match self.peek() {
                Token::LParen => self.pos += 1,
                // SuperProperty read `super.x` (no call): encoded as a
                // Member off the `__superbase__` marker, same contract
                // as the element form above. Assignment targets keep
                // the same spelling — the desugar decides read vs
                // write semantics from the surrounding node.
                _ => {
                    let marker = self.ast.add_expr(Expr::Ident("__superbase__".to_string()));
                    return Ok(self.ast.add_expr(Expr::Member {
                        obj: marker,
                        name: m_name,
                    }));
                }
            }
            let mut args: Vec<ExprId> = Vec::new();
            if !matches!(self.peek(), Token::RParen) {
                args.push(self.parse_call_arg()?);
                while matches!(self.peek(), Token::Comma) {
                    self.pos += 1;
                    args.push(self.parse_call_arg()?);
                }
            }
            match self.peek() {
                Token::RParen => self.pos += 1,
                t => return Err(format!("expected `)`, got {t:?} at {}", self.at())),
            }
            let args = self.fold_static_spread(args);
            let callee = self
                .ast
                .add_expr(Expr::Ident(format!("__supercall__{m_name}")));
            return Ok(self.ast.add_expr(Expr::Call { callee, args }));
        }
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `(` after `super`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // P-SURF S2.9 — ES §15.7.1 early error: a SuperCall is legal
        // only in the constructor of a class that has an `extends`
        // clause. Refusing it here rather than downstream is what makes
        // the diagnostic true: the desugar only ever rewrote `super()`
        // inside a ctor body, so every other position used to reach the
        // checker as `super(...) reached check.rs (desugar didn't run?)`
        // — an internal note blaming a pass that had in fact run.
        if !self.super_call_allowed {
            return Err(format!(
                "`super()` is only valid in the constructor of a class with an \
                 `extends` clause, at {}",
                self.at()
            ));
        }
        // Ctor args ride the spread-aware arg parser like plain calls
        // and `new` (chunk 684 shape): a static literal spread folds
        // to fixed args here, a dynamic spread survives as
        // Expr::Spread for `apply_spread_args` to desugar AFTER the
        // class pass has rewritten this site into a plain
        // `__cm_<Parent>__ctor(...)` call.
        let mut args: Vec<ExprId> = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            args.push(self.parse_call_arg()?);
            while matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                args.push(self.parse_call_arg()?);
            }
        }
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => return Err(format!("expected `)`, got {t:?} at {}", self.at())),
        }
        let args = self.fold_static_spread(args);
        Ok(self.ast.add_expr(Expr::Super { args }))
    }

    pub(super) fn parse_primary_new(&mut self) -> Result<ExprId, String> {
        // `new ClassName(args)` — type args / generic ctors not yet
        // supported; that's M5.2 alongside extends.
        self.pos += 1;
        // P4.5 — `new.target` meta-property. Spec §13.3.10:
        // evaluates to the [[NewTarget]] of the current
        // execution context. Recognized at the parser layer
        // because `new` followed by `.` would otherwise hit the
        // class-name error path.
        if matches!(self.peek(), Token::Dot) {
            self.pos += 1;
            match self.peek() {
                Token::Ident(n) if n == "target" => {
                    self.pos += 1;
                    return Ok(self.ast.add_expr(Expr::NewTarget));
                }
                t => {
                    return Err(format!(
                        "expected `target` after `new.`, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        }
        let head = match self.peek() {
            Token::Ident(n) => {
                let n = n.clone();
                self.pos += 1;
                // P8.5 — narrow alias resolution. If `new F()`
                // and F is a const-bound class expression,
                // rewrite to `new __ClassExpr_<id>()` so the
                // static-factory path resolves to the synth
                // class's `__new___ClassExpr_<id>`. Avoids a
                // dynamic-ctor-dispatch substrate change.
                NewHead::Named(self.class_value_aliases.get(&n).cloned().unwrap_or(n))
            }
            // P8.5 — `new class { ... }(args)` /
            // `new class Foo { ... }(args)`. Parse the class
            // expression inline as a ClassDecl with synth name,
            // buffer to synth_classes, and use the synth name
            // as the new target. parse_class_decl_with_abstract
            // consumes `class` + body itself.
            Token::Class => {
                let stmt = self.parse_class_decl_with_abstract(false, true, true)?;
                // `new class x extends x {}` — the definition would
                // throw the TDZ ReferenceError before `new` ever ran;
                // the NewHead machinery is name-based and cannot carry
                // a throwing expression, so this stays a loud reject
                // (class_self_heritage covers the decl/expr forms).
                if let Some(name) = super::class_self_heritage::expr_self_extends(&self.ast, &stmt)
                {
                    return Err(format!(
                        "`new` on a class expression extending its own name `{name}` \
                         (a TDZ ReferenceError at runtime) is not supported at {}",
                        self.at()
                    ));
                }
                let cls_name = match &stmt {
                    Stmt::ClassDecl { name, .. } => name.clone(),
                    _ => unreachable!(),
                };
                // 393-01 — same buffer split as parse_primary_class_expr:
                // outside a class body the decl lands next to its use
                // site so a capturing `new class { … }()` reaches the
                // nested-class machinery instead of silently losing its
                // scope to the top-level splice.
                if !self.class_stack.is_empty() {
                    self.synth_classes.push(stmt);
                } else {
                    self.synth_classes_local.push(stmt);
                }
                NewHead::Named(cls_name)
            }
            // P8.5 — `new (expr)(args)` per ES spec §13.3.5
            // NewExpression: the callee may be a parenthesized
            // expression. An inner Ident keeps the static path —
            // that covers `new (class { ... })()` (the class-
            // expression branch in parse_primary emits an Ident at
            // that inner site) and `new (SomeClass)()`. Anything
            // else is a constructor the parser cannot name, so it
            // goes to the runtime-construct path.
            Token::LParen => {
                self.pos += 1;
                let inner = self.parse_expr()?;
                match self.peek() {
                    Token::RParen => self.pos += 1,
                    t => {
                        return Err(format!(
                            "expected `)` after `new (...`, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
                match self.ast.get_expr(inner) {
                    Expr::Ident(n) => NewHead::Named(n.clone()),
                    // `new (E as SomeType)(args)` — the TS `as` cast is a
                    // static-only assertion; the runtime callee is still
                    // the inner expression. Peel it here so the downstream
                    // class-registry lookup (P8.5 `new C(args)`) sees `E`.
                    Expr::As { expr, .. } => match self.ast.get_expr(*expr) {
                        Expr::Ident(n) => NewHead::Named(n.clone()),
                        _ => NewHead::Dynamic(inner),
                    },
                    _ => NewHead::Dynamic(inner),
                }
            }
            // Cluster #6 (rotation 438) — ES §13.3.5: the callee of a
            // NewExpression is a MemberExpression, so ANY primary is
            // legal here (`new true`, `new 1`, `new function(){}`,
            // `new /z/`, a nested `new X`). Whether it is a
            // constructor is §7.2.4 IsConstructor — a RUNTIME
            // question `__torajs_anyv_construct` answers with a
            // TypeError. Parse the primary and ride the
            // dynamic-construct path; `.`/`[` tails extend below.
            _ => NewHead::Dynamic(self.parse_primary()?),
        };
        // ES §13.3 NewExpression: what follows `new` is a
        // MemberExpression, so a `.` or `[` here is part of the
        // callee — `new a.b()` constructs `a.b`, not `a`. Reading
        // only the head identifier used to turn that into
        // `(new a).b()`, a different program with no complaint.
        let head = self.extend_new_callee(head)?;
        // Explicit TS instantiation on built-in generics:
        // `new Set<number>()` / `new Map<string, number>()`. Parsed
        // into flat ann spellings and carried on `Expr::New.type_args`
        // (callback-param seeding reads the container's element types
        // from here); no mono-instantiation happens downstream yet.
        // The `type_ann_depth` guard keeps the spellings out of
        // `type_ann_spans` — fn_source_erase never spliced the old
        // skip form either, so toString output stays unchanged.
        let mut type_args: Vec<String> = Vec::new();
        if matches!(self.peek(), Token::Lt) {
            self.pos += 1;
            self.type_ann_depth += 1;
            let parsed = self.parse_type_args_list();
            self.type_ann_depth -= 1;
            type_args = parsed?;
        }
        // V3-18 m1.h.22 — JS spec §13.3.5 NewExpression
        // permits `new Foo` (no parens), equivalent to
        // `new Foo()`. Test262 uses both forms; the no-
        // parens form previously hard-rejected.
        let has_parens = matches!(self.peek(), Token::LParen);
        if has_parens {
            self.pos += 1;
        }
        // Ctor args ride the same spread-aware arg parser as plain
        // calls (chunk 684): `...` wraps in Expr::Spread, a static
        // literal spread (`new C(...[1, 2])`) folds to fixed args
        // here, and a dynamic trailing spread (`new C(...arr)`) is
        // desugared by `apply_spread_args`' New arm.
        let mut args: Vec<ExprId> = Vec::new();
        if has_parens && !matches!(self.peek(), Token::RParen) {
            args.push(self.parse_call_arg()?);
            while matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                args.push(self.parse_call_arg()?);
            }
        }
        if has_parens {
            match self.peek() {
                Token::RParen => self.pos += 1,
                t => return Err(format!("expected `)`, got {t:?} at {}", self.at())),
            }
        }
        let args = self.fold_static_spread(args);
        Ok(match head {
            NewHead::Named(class_name) => self.ast.add_expr(Expr::New {
                class_name,
                args,
                type_args,
            }),
            // `type_args` is dropped here on purpose: it only feeds
            // callback-param seeding off a named container class, and
            // there is no name to seed from.
            NewHead::Dynamic(callee) => self.ast.add_expr(Expr::NewDynamic { callee, args }),
        })
    }

    /// Consume the `.name` / `[expr]` tail of a `new` callee, if any.
    ///
    /// Reaching one at all means the constructor is an expression
    /// rather than a name, so a named head is materialized back into
    /// an `Ident` and the result is dynamic from here on.
    fn extend_new_callee(&mut self, head: NewHead) -> Result<NewHead, String> {
        if !matches!(self.peek(), Token::Dot | Token::LBracket) {
            return Ok(head);
        }
        let start_pos = self.pos;
        let mut node = match head {
            NewHead::Dynamic(e) => e,
            NewHead::Named(n) => self.ast.add_expr(Expr::Ident(n)),
        };
        loop {
            match self.peek() {
                Token::Dot => {
                    self.pos += 1;
                    let name = self.expect_member_name(".")?;
                    node = self.add_expr_at(start_pos, Expr::Member { obj: node, name });
                }
                Token::LBracket => node = self.parse_postfix_index(node, start_pos)?,
                _ => return Ok(NewHead::Dynamic(node)),
            }
        }
    }
}
