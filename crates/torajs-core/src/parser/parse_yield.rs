//! `yield` / `yield*` statement parsing — split from `parse_stmt.rs`
//! (S2.28 grew the `yield*` arm past that file's 500-line headroom).
//!
//! Two `yield*` delegation lanes (§27.5.3.2):
//! - **J.3 typed lane** — `yield* g(args)` where `g` is a known
//!   `function*` declaration desugars to the `__Gen_<g>` typed
//!   iterator-protocol while-loop (parse-time, fully typed).
//! - **S2.28 array lane** — `yield* [a, b, ...]` over an array
//!   literal. An array's default iterator visits index order up to
//!   `length` (§23.1.5.1), so the delegation is an indexed
//!   while+yield — the exact loop shape the generator state machine
//!   already lowers (the J.3 expansion is its sibling). Elements ride
//!   the `any` tier (`any[]` src ann), matching the P10.7 default.
//!
//! Every other operand shape (bare identifiers, calls to non-generator
//! values, strings) stays a loud parse error: an indexed expansion
//! over a non-array iterable would silently yield nothing (`.length`
//! undefined), and the general GetIterator protocol needs a runtime
//! iterator surface the any lane does not carry yet (L3b).

use super::*;

impl<'a> Parser<'a> {
    /// `yield e ;` / `yield ;` / `yield * gen(args) ;` /
    /// `yield * [elems] ;` statement. Caller has peeked `Token::Yield`.
    pub(super) fn parse_yield_stmt(&mut self) -> Result<Stmt, String> {
        // §15.5.5 / §16.1 early error (r290) — same parse-time gate
        // as the expression-position twin (yield_expr_hoist.rs): the
        // checker's own reject fires too late for shapes the resolver
        // walks first.
        if !self.in_generator {
            return Err(format!(
                "`yield` is only valid inside a `function*` generator body at {} (ES §15.5.5)",
                self.at()
            ));
        }
        self.pos += 1;
        if matches!(self.peek(), Token::Star) {
            self.pos += 1;
            let src = self.parse_expr()?;
            if matches!(self.peek(), Token::Semi) {
                self.pos += 1;
            }
            if matches!(self.ast.get_expr(src), Expr::Array(_)) {
                return Ok(self.emit_yieldstar_array(src));
            }
            // J.3 typed lane — a direct call to a known function*
            // keeps the fully-typed expansion. An async generator
            // body skips it (F3): the inner async gen's next()
            // answers Promise<__step>, which the typed expansion
            // reads unsettled — the generic for-await desugar below
            // carries the await drive instead.
            let known_gen_call = matches!(
                self.ast.get_expr(src),
                Expr::Call { callee, .. }
                    if matches!(self.ast.get_expr(*callee),
                        Expr::Ident(n) if self.generator_fns.contains_key(n))
            );
            if known_gen_call && !self.in_async_gen {
                return self.emit_yieldstar_known(src);
            }
            return Ok(self.emit_yieldstar_generic(src));
        }
        let v = self.parse_yield_operand()?;
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        Ok(Stmt::Yield(v))
    }

    /// S2.41 — YieldExpression's operand is OPTIONAL (§15.5.5:
    /// `yield` [no LineTerminator] AssignmentExpression?): a bare
    /// `yield` before an expression terminator yields undefined (the
    /// t262 for-of / for-await-of dstr suites drive their generators
    /// with `yield;`). The pre-S2.41 parse handed the terminator to
    /// `parse_expr` — "expected expression, got Semi".
    pub(super) fn parse_yield_operand(&mut self) -> Result<ExprId, String> {
        if matches!(
            self.peek(),
            Token::Semi | Token::RBrace | Token::RParen | Token::RBracket | Token::Comma
        ) {
            return Ok(self.ast.add_expr(Expr::Ident("undefined".into())));
        }
        self.parse_expr()
    }

    /// S2.28 — `yield* [a, b, ...]` array-literal delegation: hoist
    /// the array into an `any[]` src binding and yield each element
    /// through an indexed while (probe-proven inside both sync and
    /// async generator state machines).
    fn emit_yieldstar_array(&mut self, src: ExprId) -> Stmt {
        let id = self.mint_desugar_id();
        let src_name = format!("__ys_src_{id}");
        let i_name = format!("__ys_i_{id}");
        let mut stmts: Vec<Stmt> = Vec::new();
        stmts.push(Stmt::LetDecl {
            mutable: false,
            name: src_name.clone(),
            type_ann: Some("any[]".into()),
            init: src,
            is_var: false,
        });
        let zero = self.ast.add_expr(Expr::Number(0.0));
        stmts.push(Stmt::LetDecl {
            mutable: true,
            name: i_name.clone(),
            type_ann: Some("number".into()),
            init: zero,
            is_var: false,
        });
        let i_ref = self.ast.add_expr(Expr::Ident(i_name.clone()));
        let src_ref = self.ast.add_expr(Expr::Ident(src_name.clone()));
        let len = self.ast.add_expr(Expr::Member {
            obj: src_ref,
            name: "length".into(),
        });
        let cond = self.ast.add_expr(Expr::BinOp {
            op: BinOp::Lt,
            left: i_ref,
            right: len,
        });
        let src_ref_elem = self.ast.add_expr(Expr::Ident(src_name));
        let i_ref_elem = self.ast.add_expr(Expr::Ident(i_name.clone()));
        let elem = self.ast.add_expr(Expr::Index {
            obj: src_ref_elem,
            index: i_ref_elem,
        });
        let yield_stmt = Stmt::Yield(elem);
        let i_ref_inc = self.ast.add_expr(Expr::Ident(i_name.clone()));
        let one = self.ast.add_expr(Expr::Number(1.0));
        let inc = self.ast.add_expr(Expr::BinOp {
            op: BinOp::Add,
            left: i_ref_inc,
            right: one,
        });
        let i_target = self.ast.add_expr(Expr::Ident(i_name));
        let assign = self.ast.add_expr(Expr::Assign {
            target: i_target,
            value: inc,
        });
        let step = Stmt::Expr(assign);
        let while_loop = Stmt::While {
            cond,
            body: Box::new(Stmt::Block(vec![yield_stmt, step])),
        };
        stmts.push(while_loop);
        Stmt::Block(stmts)
    }

    /// Expression-position `yield* src` (§27.5.3.2 done completion).
    /// Caller (`parse_yield_expr_hoist`) has consumed `yield` and `*`
    /// and parsed the operand. Hoists two statements into
    /// `yield_hoist_buf`:
    ///
    /// ```text
    /// let __yx_<n>: any = undefined;          // mutable done-value temp
    /// for (const __ysxv_<n> of src) { yield __ysxv_<n> }
    /// ```
    ///
    /// and yields back `Ident(__yx_<n>)`. The element name is
    /// registered in `yieldstar_done_targets`, so the F1 manual
    /// protocol (desugar_generators_forof) writes `__step.value` into
    /// the temp on the done step instead of dropping it — that final
    /// value IS the yield* expression's value. The `__yx_` prefix
    /// keeps the temp inside the eval-order guard's and the
    /// assignment-target reject's existing namespace.
    pub(super) fn emit_yieldstar_expr_hoist(&mut self, src: ExprId) -> ExprId {
        let id = self.mint_desugar_id();
        let done_var = format!("__yx_{id}");
        let v_name = format!("__ysxv_{id}");
        let undef = self.ast.add_expr(Expr::Ident("undefined".into()));
        self.yield_hoist_buf.push(Stmt::LetDecl {
            mutable: true,
            name: done_var.clone(),
            type_ann: Some("any".into()),
            init: undef,
            is_var: false,
        });
        let v_ref = self.ast.add_expr(Expr::Ident(v_name.clone()));
        let body = Stmt::Block(vec![Stmt::Yield(v_ref)]);
        let is_async = self.in_async_gen;
        self.ast
            .yieldstar_done_targets
            .insert(v_name.clone(), done_var.clone());
        let forof = self.emit_forof_default(v_name, Some("any".into()), src, body, is_async, None);
        self.yield_hoist_buf.push(forof);
        self.ast.add_expr(Expr::Ident(done_var))
    }

    /// F2 (RFC 20260728-gen-forof-yieldstar) — generic `yield* e`
    /// delegation (§27.5.3.2 next-drive): desugars to
    /// `for (const __ysv_N of e) { yield __ysv_N }`; the generator
    /// prep's F1 pass turns that into the manual iterator protocol.
    /// In an async generator (F3) the ForOf carries `is_await`, so
    /// the F1 rewrite drives it with the §14.7.5.6 await taps
    /// (§27.5.3.2 with generatorKind=async — steps 6.a.viii/7.a).
    /// Residual (registered): received `throw()` / `return()`
    /// forwarding to the inner iterator (spec steps 7.b/7.c) and the
    /// done-completion value.
    fn emit_yieldstar_generic(&mut self, src: ExprId) -> Stmt {
        let id = self.mint_desugar_id();
        let v_name = format!("__ysv_{id}");
        let v_ref = self.ast.add_expr(Expr::Ident(v_name.clone()));
        let body = Stmt::Block(vec![Stmt::Yield(v_ref)]);
        let is_async = self.in_async_gen;
        self.emit_forof_default(v_name, Some("any".into()), src, body, is_async, None)
    }

    /// J.3 — `yield * gen(args);` delegates to an inner generator.
    /// Parse-time desugar: same iterator-protocol shape as for-of
    /// over a known generator factory, with `yield __step.value`
    /// as the loop body. Non-call / unknown-callee operands error
    /// loudly here (module doc).
    fn emit_yieldstar_known(&mut self, src: ExprId) -> Result<Stmt, String> {
        let (callee_name, yield_ty) = match self.ast.get_expr(src) {
            Expr::Call { callee, .. } => match self.ast.get_expr(*callee) {
                Expr::Ident(n) => match self.generator_fns.get(n).cloned() {
                    Some(yt) => (n.clone(), yt),
                    None => {
                        return Err(format!(
                            "yield* `{n}(...)` — `{n}` is not a known function* \
                             declaration (J.3 MVP only handles direct generator \
                             factory calls) at {}",
                            self.at()
                        ));
                    }
                },
                _ => {
                    return Err(format!(
                        "yield* requires a direct call to a function* declaration \
                         (got non-ident callee) at {}",
                        self.at()
                    ));
                }
            },
            _ => {
                return Err(format!(
                    "yield* requires a direct call to a function* declaration \
                     (got non-call expr) at {}",
                    self.at()
                ));
            }
        };
        let gen_class = format!("__Gen_{callee_name}");
        let step_ty = format!("__step_{callee_name}");
        let id = self.mint_desugar_id();
        let it_name = format!("__yieldstar_it_{id}");
        let step_name = format!("__yieldstar_step_{id}");

        let mut stmts: Vec<Stmt> = Vec::new();
        stmts.push(Stmt::LetDecl {
            mutable: false,
            name: it_name.clone(),
            type_ann: Some(gen_class),
            init: src,
            is_var: false,
        });

        let it_ref = self.ast.add_expr(Expr::Ident(it_name));
        let next_member = self.ast.add_expr(Expr::Member {
            obj: it_ref,
            name: "next".into(),
        });
        let next_call = self.ast.add_expr(Expr::Call {
            callee: next_member,
            args: Vec::new(),
        });
        let step_decl = Stmt::LetDecl {
            mutable: false,
            name: step_name.clone(),
            type_ann: Some(step_ty),
            init: next_call,
            is_var: false,
        };

        let step_ref_done = self.ast.add_expr(Expr::Ident(step_name.clone()));
        let done_member = self.ast.add_expr(Expr::Member {
            obj: step_ref_done,
            name: "done".into(),
        });
        let done_check = Stmt::If {
            cond: done_member,
            then_branch: Box::new(Stmt::Break(None)),
            else_branch: None,
        };

        let step_ref_value = self.ast.add_expr(Expr::Ident(step_name));
        let value_member = self.ast.add_expr(Expr::Member {
            obj: step_ref_value,
            name: "value".into(),
        });
        let _ = yield_ty; // type ann implicit via yield expr
        let yield_stmt = Stmt::Yield(value_member);

        let loop_body = Stmt::Block(vec![step_decl, done_check, yield_stmt]);
        let true_lit = self.ast.add_expr(Expr::Bool(true));
        let while_loop = Stmt::While {
            cond: true_lit,
            body: Box::new(loop_body),
        };
        stmts.push(while_loop);
        Ok(Stmt::Block(stmts))
    }
}
