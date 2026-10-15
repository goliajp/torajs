//! parse_primary atom arms (chunk 422).
//!
//! Extracted verbatim (dedented one level where noted) from
//! parse_primary (parser.rs):
//! - parse_primary_dyn_import — dynamic `import("./y")` (P13-S5)
//! - parse_primary_paren — `(e)` / comma operator `(a, b, c)` /
//!   arrow-fn disambiguation at `(`
//! - parse_primary_class_expr — class expression (P8.5 synth-name
//!   ClassDecl buffering)
//! - try_parse_bare_arrow — single-param arrow `x => body` probe;
//!   Ok(None) when the Ident is not followed by `=>` (the only
//!   non-verbatim edit is its Ok(expr) -> Ok(Some(expr)) tail wrap)
//!
//! The first three assume the cursor sits at `import` / `(` / `class`;
//! parse_primary's dispatch arms delegate here. Bodies unchanged.

use super::*;

impl<'a> Parser<'a> {
    // P13-S5 — dynamic `import(<expr>)` expression. §13.3.10 takes a
    // full AssignmentExpression; every call site — string literal or
    // not — rewrites to `__torajs_dyn_import(<expr>)`, a dispatcher
    // FnDecl the module resolver synthesizes. The resolver bakes a
    // candidate table (source string literals that resolve to module
    // files, webpack context-module style, narrowed to exact
    // literals) via the same namespace-injection machinery the static
    // `import * as ns` form uses; at runtime the dispatcher does
    // ToString(specifier) — an abrupt completion rejects the promise
    // per step 7 — then matches the baked table, and a miss rejects.
    pub(super) fn parse_primary_dyn_import(&mut self) -> Result<ExprId, String> {
        self.pos += 1;
        // import-defer proposal — `import.defer("...")` is the
        // dynamic phase-import form. The eager AOT subset resolves
        // it like a plain `import(...)` (deferred evaluation is
        // layered — same posture as the `import defer * as ns`
        // declaration). `import.source` stays a loud reject: it
        // answers a ModuleSource object, which needs a module-
        // reflection substrate no eager rewrite can fake.
        if matches!(self.peek(), Token::Dot) {
            match &self.tokens[self.pos + 1].token {
                Token::Ident(n) if n == "defer" => {
                    self.pos += 2; // consume `.` + `defer`
                }
                Token::Ident(n) if n == "source" => {
                    return Err(format!(
                        "`import.source` needs a ModuleSource substrate \
                         (module-reflection proposal) at {}",
                        self.at()
                    ));
                }
                _ => {} // fall through — the LParen check rejects
            }
        }
        if !matches!(self.peek(), Token::LParen) {
            return Err(format!(
                "dynamic `import` requires `(<source>)`; got {:?} at {}",
                self.peek(),
                self.at()
            ));
        }
        self.pos += 1;
        // `import()` with no argument — the AssignmentExpression is
        // not optional (test262 syntax/invalid *-not-optional).
        if matches!(self.peek(), Token::RParen) {
            return Err(format!(
                "dynamic `import` requires an argument (§13.3.10 \
                 ImportCall's AssignmentExpression is not optional) at {}",
                self.at()
            ));
        }
        // ImportCall's arguments are AssignmentExpression[+In]: a
        // for-init `in` restriction does not reach inside the parens.
        let saved_in_for_init = std::mem::replace(&mut self.in_for_init, false);
        let spec = self.parse_expr()?;
        // §13.3.10 ImportCall's optional second argument (import
        // options), evaluated after the specifier and handed to the
        // dispatcher, which validates it after ToString(specifier).
        // Trailing commas are grammar-legal in both the 1-arg and
        // 2-arg forms.
        let mut options = None;
        if matches!(self.peek(), Token::Comma) {
            self.pos += 1;
            if !matches!(self.peek(), Token::RParen) {
                options = Some(self.parse_expr()?);
                if matches!(self.peek(), Token::Comma) {
                    self.pos += 1;
                }
            }
        }
        self.in_for_init = saved_in_for_init;
        if !matches!(self.peek(), Token::RParen) {
            return Err(format!(
                "expected `)` to close dynamic `import(...)`, got {:?} at {}",
                self.peek(),
                self.at()
            ));
        }
        self.pos += 1;
        self.ast.dyn_import_present = true;
        let callee = self
            .ast
            .add_expr(Expr::Ident("__torajs_dyn_import".to_string()));
        let options =
            options.unwrap_or_else(|| self.ast.add_expr(Expr::Ident("undefined".to_string())));
        Ok(self.ast.add_expr(Expr::Call {
            callee,
            args: vec![spec, options],
        }))
    }

    pub(super) fn parse_primary_paren(&mut self) -> Result<ExprId, String> {
        // `(` could start either a parenthesized expression `(e)` or an
        // arrow fn `(params) =>`. Disambiguate by scanning forward to the
        // matching `)` and peeking for `=>`.
        if self.is_arrow_fn_at_lparen() {
            return self.parse_arrow_fn();
        }
        self.pos += 1; // consume `(`
        // §14.7.4 — parentheses reset the [In] restriction: a
        // for-head `for ((#x in o) ? a : b;;)` is legal inside them.
        let saved_in_for_init = std::mem::replace(&mut self.in_for_init, false);
        // §13.16 comma operator: `(a, b, c)` evaluates left to right
        // and answers the rightmost value.
        let last = self.parse_comma_list()?;
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after parenthesized expression, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        self.in_for_init = saved_in_for_init;
        if matches!(
            self.ast.get_expr(last),
            Expr::ObjectLit { .. } | Expr::Array(_)
        ) {
            self.invalid_target_ids.insert(last.0);
        }
        // §13.3.9: the parentheses end an optional chain
        let mut link = last;
        while let Expr::Member { obj, .. } = self.ast.get_expr(link) {
            link = *obj;
        }
        if matches!(self.ast.get_expr(link), Expr::OptChain { .. }) {
            self.ast.paren_optchains.insert(last);
        }
        Ok(last)
    }

    // P8.5 — class expression in expression position
    // (ES spec §15.7.4 ClassExpression). Forms:
    //   const F = class { ... }                  // anonymous
    //   const F = class Inner { ... }            // named (inner
    //                                            //   self-binding
    //                                            //   not yet wired)
    //   const F = class extends Parent { ... }   // extends
    //   foo(class { ... })                       // class as argument
    // Strategy (a): parse as a ClassDecl with an `__ClassExpr_<id>`
    // synth name, buffer it (`buffer_class_expr`) for a splice in
    // front of the statement that uses it, then emit
    // `Expr::Ident("__ClassExpr_<id>")` at the original use site.
    // The existing class machinery (desugar_classes,
    // synthesize_class_globals) lifts this Ident to the value-form
    // class global with zero changes downstream. Spec deviations
    // documented as L3b: anonymous `.name === "__ClassExpr_<id>"`
    // (should be `""`); named `Inner` self-binding inside body
    // doesn't resolve to the synth name.
    pub(super) fn parse_primary_class_expr(&mut self) -> Result<ExprId, String> {
        let stmt = self.parse_class_decl_with_abstract(false, true, true)?;
        // `class x extends x {}` in expression position — the TDZ
        // ReferenceError shape (class_self_heritage): the decl never
        // reaches synth_classes, the use site throws when evaluated.
        if let Some(name) = super::class_self_heritage::expr_self_extends(&self.ast, &stmt) {
            return Ok(self.class_expr_self_tdz(&name));
        }
        let cls_name = match &stmt {
            Stmt::ClassDecl { name, .. } => name.clone(),
            _ => unreachable!("parse_class_decl_with_abstract returns ClassDecl"),
        };
        Ok(self.buffer_class_expr(stmt, &cls_name))
    }

    /// Where a freshly minted class expression's ClassDecl lands.
    /// Outside any class body (393-01), or inside a STATEMENT of a
    /// member body (K6-1: `stmt_depth` is deeper than it was at the
    /// innermost body's entry — a method / ctor / accessor / static
    /// block statement, or a block-bodied arrow written in one), the
    /// decl goes NEXT TO its use site: the `parse_stmt` wrapper drains
    /// `synth_classes_local` in front of the finished statement, so
    /// the class stays a nested ClassDecl in the scope it was written
    /// in and `hoist_nested_classes` decides its lane (capture-free →
    /// a unit class per evaluation; capturing → the runtime-value
    /// lane, where an outer `self` is a closure capture).
    ///
    /// An INSTANCE field initializer counts as a member statement
    /// too: it runs per construction inside the ctor, and the decl
    /// rides into the ctor prefix with it (`field_init_classes`).
    ///
    /// At the body's own depth otherwise — a static field
    /// initializer, a computed key, an expression-bodied arrow in
    /// either — a class written inside a function or block body
    /// becomes the body of an immediately invoked arrow
    /// ([`Self::class_expr_iife`]). A top-level class keeps the
    /// top-level splice of rotation 373: it evaluates when THAT code
    /// runs, after every top-level class definition completed, so it
    /// is marked deferred and the field-flattening order check lets
    /// it extend the enclosing class.
    ///
    /// Answers the expression the use site evaluates.
    pub(super) fn buffer_class_expr(&mut self, stmt: Stmt, cls_name: &str) -> ExprId {
        let in_member_stmt = self.in_instance_field_init
            || self
                .class_body_stmt_depths
                .last()
                .is_none_or(|&entry| self.stmt_depth > entry);
        if in_member_stmt {
            self.synth_classes_local.push(stmt);
        } else if self
            .class_body_stmt_depths
            .last()
            .is_some_and(|&entry| entry > 1)
        {
            return self.class_expr_iife(stmt, cls_name);
        } else {
            self.ast.class_expr_deferred.insert(cls_name.to_string());
            self.synth_classes.push(stmt);
        }
        self.ast.add_expr(Expr::Ident(cls_name.to_string()))
    }

    /// K6 residue — a class expression at a NESTED class's own body
    /// depth (a static field initializer or a computed key; the class
    /// body entered below the top-level statement) evaluates when that
    /// code runs, with the enclosing scope in reach (§15.7.14). The
    /// top-level splice gave it neither: `static I = class { m() {
    /// return k } }` inside `function outer(k)` read an unbound `k`.
    /// As the body of an immediately invoked arrow it is a class
    /// declared in a function body, which `hoist_nested_classes`
    /// already evaluates in place and lets capture; the arrow keeps
    /// the initializer's `this`.
    fn class_expr_iife(&mut self, stmt: Stmt, cls_name: &str) -> ExprId {
        let ret = self.ast.add_expr(Expr::Ident(cls_name.to_string()));
        let arrow = self.ast.add_expr(Expr::ArrowFn {
            params: Vec::new(),
            return_type: None,
            body: vec![stmt, Stmt::Return(Some(ret))],
        });
        let call = self.ast.add_expr(Expr::Call {
            callee: arrow,
            args: Vec::new(),
        });
        self.ast
            .class_expr_iife_calls
            .insert(call, cls_name.to_string());
        call
    }

    /* Single-param arrow without parens: `x => body`. JS spec
     * accepts this as shorthand for `(x) => body` (the `x` has
     * no type annotation; tr's check.rs auto-promotes untyped
     * params to fresh type params via desugar_implicit_generics).
     * Detect by peeking Ident + FatArrow before falling through
     * to the regular Ident expression. */
    pub(super) fn try_parse_bare_arrow(&mut self) -> Result<Option<ExprId>, String> {
        let pos = self.pos;
        // §15.3 ArrowParameters : BindingIdentifier — a contextual
        // keyword names one like any other word, so the FatArrow is
        // confirmed FIRST and the admission asked second (asking it
        // anywhere else would record an `await` site that no arrow
        // follows).
        // §15.3 ArrowFunction : ArrowParameters [no LineTerminator
        // here] `=>` — a newline before the arrow is not an arrow at
        // all, and `async \n => 0` is the shape that shows it.
        let next_is_arrow = self
            .tokens
            .get(pos + 1)
            .map(|t| matches!(t.token, Token::FatArrow))
            .unwrap_or(false)
            && !self.has_newline_before(pos + 1);
        if next_is_arrow
            && let Some(pname) = self.in_non_lexical_position(|p| p.pattern_binding_name())
        {
            self.pos += 2; /* consume Ident + FatArrow */
            // §15.3 — a non-async ArrowFunction's body is
            // [~Await], the same context `parse_arrow_fn` gives
            // the parenthesized form. Without it `await => await
            // + 1` read its own parameter as an AwaitExpression
            // and answered 1 for every argument.
            let saved_await = std::mem::replace(&mut self.await_allowed, false);
            let saved_await_res = std::mem::replace(&mut self.await_reserved, false);
            let body_result = self.parse_bare_arrow_body();
            self.await_allowed = saved_await;
            self.await_reserved = saved_await_res;
            let body = body_result?;
            self.judge_bare_arrow_param(&pname, &body)?;
            return Ok(Some(self.add_expr_at(
                pos,
                Expr::ArrowFn {
                    params: vec![Param {
                        name: pname,
                        type_ann: None,
                        default: None,
                        is_rest: false,
                    }],
                    return_type: None,
                    body,
                },
            )));
        }
        Ok(None)
    }

    /// The body half of [`Self::try_parse_bare_arrow`], split out so
    /// the `[~Await]` context its caller installs is restored on the
    /// error paths too — the same save / call / restore / `?` order
    /// `parse_arrow_fn` uses for the parenthesized form.
    fn parse_bare_arrow_body(&mut self) -> Result<Vec<Stmt>, String> {
        Ok(if matches!(self.peek(), Token::LBrace) {
            self.pos += 1;
            let mut stmts = Vec::new();
            while !matches!(self.peek(), Token::RBrace | Token::Eof) {
                stmts.push(self.parse_stmt()?);
            }
            match self.peek() {
                Token::RBrace => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `}}` after arrow body, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            stmts
        } else {
            // Watermark drain (406-01) — a class expression
            // minted while this expression body parsed lands
            // inside the body, not in front of the enclosing
            // statement. Same protocol as parse_arrow_fn.
            let synth_mark = self.synth_classes_local.len();
            let e = self.parse_expr()?;
            let mut v = self.synth_classes_local.split_off(synth_mark);
            v.push(Stmt::Return(Some(e)));
            v
        })
    }
}
