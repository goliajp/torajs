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
    // P13-S5 — dynamic `import("./y")` expression. Restricted to a
    // string-literal source (compile-time resolvable for AOT) so we
    // can synthesize a `import * as __dyn_<n> from "./y"` decl in
    // the same translation unit and rewrite the expression to
    // `Promise.resolve(__dyn_<n>)`. The runtime path is identical
    // to the static namespace-import form (P13-S2) — the only
    // observable difference is the `Promise<…>` wrapper, matching
    // `import()` per ES §13.3.10.
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
        let source = match self.peek() {
            Token::String(s) => {
                let s = s.clone();
                self.pos += 1;
                s
            }
            t => {
                return Err(format!(
                    "dynamic `import` requires a string-literal source; got {t:?} at {}",
                    self.at()
                ));
            }
        };
        // §13.3.10 ImportCall's optional second argument (import
        // options). Parsed and DISCARDED — the eager AOT subset
        // resolves the module at compile time, so options cannot
        // change linking (their evaluation side effects are a
        // recorded subset boundary). Trailing commas are grammar-
        // legal in both the 1-arg and 2-arg forms.
        if matches!(self.peek(), Token::Comma) {
            self.pos += 1;
            if !matches!(self.peek(), Token::RParen) {
                let _ = self.parse_expr()?;
                if matches!(self.peek(), Token::Comma) {
                    self.pos += 1;
                }
            }
        }
        if !matches!(self.peek(), Token::RParen) {
            return Err(format!(
                "expected `)` to close dynamic `import(...)`, got {:?} at {}",
                self.peek(),
                self.at()
            ));
        }
        self.pos += 1;
        let n = self.dyn_import_counter;
        self.dyn_import_counter += 1;
        let ns_name = format!("__dyn_ns_{n}");
        // Synthesize: `import * as <ns_name> from <source>`. The K.2
        // resolver injects `source`'s exports as top-level decls and
        // rewrites the `Ident(<ns_name>)` below into the namespace
        // object literal IN PLACE (`inline_dyn_ns_objlits`) — no
        // top-level `let`, so the expression works inside (lifted)
        // async fn bodies where a main-local binding is invisible.
        self.synth_classes.push(Stmt::ImportDecl {
            default: None,
            namespace: Some(ns_name.clone()),
            named: Vec::new(),
            source,
        });
        // The expression is `Promise.resolve(<ns>)` — a REAL Promise
        // per §13.3.10, so both consumer shapes work: `await
        // import(...)` unwraps it and `import(...).then(cb)` chains
        // (rotation 288; the bare-namespace form this replaces made
        // `await` an identity pass but left `.then()` a typecheck
        // error — the top dynamic-import signature after the
        // statement-position dispatch fix).
        let ns_ident = self.ast.add_expr(Expr::Ident(ns_name));
        let promise_ident = self.ast.add_expr(Expr::Ident("Promise".to_string()));
        // §13.3.10 builds the promise through the intrinsic, not
        // through whatever the name `Promise` resolves to here, so no
        // `with` object may supply this one.
        self.ast.synth_global_refs.insert(promise_ident);
        let resolve = self.ast.add_expr(Expr::Member {
            obj: promise_ident,
            name: "resolve".to_string(),
        });
        return Ok(self.ast.add_expr(Expr::Call {
            callee: resolve,
            args: vec![ns_ident],
        }));
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
        // V3-18 m1.h.6 — JS spec §13.16 comma operator inside
        // parentheses: `(a, b, c)` evaluates left-to-right and
        // returns the rightmost value. Earlier subexpressions
        // are still type-checked for side effects but their
        // values are discarded. Encoded as nested
        // Expr::Sequence (using a temp let + discard pattern at
        // lower time) — for the MVP we simply discard left
        // values and return the rightmost expression's id.
        let mut last = self.parse_expr()?;
        while matches!(self.peek(), Token::Comma) {
            self.pos += 1;
            let next = self.parse_expr()?;
            last = self.ast.add_expr(Expr::Sequence {
                left: last,
                right: next,
            });
        }
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
        return Ok(last);
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
    // synth name, buffer to `synth_classes` for splice immediately
    // before this stmt's push in `parse_program`, then emit
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
        // Rotation 373 (L3b 373-05) — an expression-position class
        // inside an enclosing class body evaluates when THAT code
        // runs, after every top-level class definition completed;
        // mark it so the field-flattening order check defers it
        // (the synth flush below lands it before the enclosing
        // ClassDecl stmt). `class_stack` is the enclosing-class-body
        // signal the private-name machinery already maintains.
        if !self.class_stack.is_empty() {
            self.ast.class_expr_deferred.insert(cls_name.clone());
            self.synth_classes.push(stmt);
        } else {
            // 393-01 — outside a class body the decl lands NEXT TO its
            // use site (the parse_stmt wrapper drains this buffer), so
            // a class expression in a nested scope keeps its scope and
            // the nested-class machinery decides its fate. The
            // class-body case stays on the top-level splice: its
            // deferral contract above is written against that.
            self.synth_classes_local.push(stmt);
        }
        return Ok(self.ast.add_expr(Expr::Ident(cls_name)));
    }

    /* Single-param arrow without parens: `x => body`. JS spec
     * accepts this as shorthand for `(x) => body` (the `x` has
     * no type annotation; tr's check.rs auto-promotes untyped
     * params to fresh type params via desugar_implicit_generics).
     * Detect by peeking Ident + FatArrow before falling through
     * to the regular Ident expression. */
    pub(super) fn try_parse_bare_arrow(&mut self) -> Result<Option<ExprId>, String> {
        let pos = self.pos;
        if let Token::Ident(n) = &self.tokens[pos].token {
            let next_is_arrow = self
                .tokens
                .get(pos + 1)
                .map(|t| matches!(t.token, Token::FatArrow))
                .unwrap_or(false);
            if next_is_arrow {
                let pname = n.clone();
                self.pos += 2; /* consume Ident + FatArrow */
                let body = if matches!(self.peek(), Token::LBrace) {
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
                    let e = self.parse_expr()?;
                    vec![Stmt::Return(Some(e))]
                };
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
        }
        Ok(None)
    }
}
