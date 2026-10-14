//! Expression-position `yield` via parse-time hoisting (RFC
//! 20260802-yield-expression-position 刀 1).
//!
//! A YieldExpression is an AssignmentExpression alternative (§15.5.5),
//! but this AST has no yield Expr variant — the generator state
//! machine consumes yields as statements (`Stmt::Yield` /
//! `Stmt::YieldInto`). Instead of threading a new variant through
//! every walker, the parser hoists: when `parse_assign` meets `yield`
//! in expression position it parses the operand, mints a
//! `__yx_<n>` temp, pushes `YieldInto { var: __yx_<n>, value }` onto
//! `yield_hoist_buf`, and yields back `Ident(__yx_<n>)`. The
//! `parse_stmt` wrapper drains the buffer in front of the finished
//! statement as a transparent `Stmt::Multi`, so the state machine
//! sees exactly the `let x = yield v` shape it already lowers.
//! Nested `yield yield e` works for free: the inner yield hoists
//! first, so buffer order IS source evaluation order.
//!
//! Two loud boundaries keep the transform honest:
//!
//! * **Conditional positions** reject at parse time
//!   (`yield_hoist_allowed == false`): loop conditions / steps,
//!   short-circuit right-hand sides (`&& || ??`), ternary branches,
//!   optional-call arguments, parameter defaults and destructuring
//!   defaults. Hoisting out of those would run the yield
//!   unconditionally where the spec runs it conditionally (or per
//!   iteration).
//! * **Evaluation-order guard**: hoisting moves the yield in front of
//!   the statement, so any side effect the source ordered BEFORE the
//!   yield (`f(a(), yield b)` — the `a()` call) would now run after
//!   it. `check_hoist_eval_order` walks the statement in evaluation
//!   order and rejects when a side-effect event (call / new / assign
//!   / post-incr / delete) precedes the last `__yx_` temp read.
//!   Member/index reads don't count as side effects (getter-with-
//!   side-effect ordering against a yield is out of subset). A temp
//!   that never appears in the statement tree (e.g. a class computed
//!   key stashed in the `class_computed_keys` side table) passes the
//!   guard — the hoist order for those equals source order by
//!   construction.

use super::*;

impl<'a> Parser<'a> {
    /// `yield [operand]` in expression position. Caller
    /// (`parse_assign`) has peeked `Token::Yield` and NOT consumed it.
    pub(super) fn parse_yield_expr_hoist(&mut self) -> Result<ExprId, String> {
        // §15.5.5 / §16.1 early error (r290) — module code is strict,
        // so `yield` outside a `function*` body is a parse-time
        // reject. It must fire HERE rather than at the checker: a
        // yield nested in an `import(...)` argument would otherwise
        // reach the resolver first and fail on path resolution
        // instead of the expected parse-phase SyntaxError.
        if !self.in_generator {
            return Err(format!(
                "`yield` is only valid inside a `function*` generator body at {} (ES §15.5.5)",
                self.at()
            ));
        }
        if self.in_formal_params {
            return Err(format!(
                "`yield` may not be used in a formal parameter list at {} (ES §15.1.2)",
                self.at()
            ));
        }
        if !self.yield_hoist_allowed {
            return Err(format!(
                "not yet supported: `yield` in a conditional expression position \
                 (optional call argument, default value) at {}",
                self.at()
            ));
        }
        self.pos += 1; // consume `yield`
        // §15.5.5 — no line terminator between `yield` and `*` (the
        // statement twin in `parse_yield.rs` says why).
        if matches!(self.peek(), Token::Star) && !self.has_newline_before(self.pos) {
            self.pos += 1; // consume `*`
            let src = self.parse_expr()?;
            return Ok(self.emit_yieldstar_expr_hoist(src));
        }
        let value = self.parse_yield_operand()?;
        let id = self.mint_desugar_id();
        let var = format!("__yx_{id}");
        self.yield_hoist_buf.push(Stmt::YieldInto {
            var: var.clone(),
            // The temp binds the RESUMPTION value (whatever the
            // driver passes to next()), not the operand — always any.
            type_ann: Some("any".into()),
            value,
        });
        Ok(self.ast.add_expr(Expr::Ident(var)))
    }

    /// Run `f` with expression-position yield hoisting disallowed
    /// (conditional evaluation position). Restores the previous state
    /// on exit, so nesting back into an allowed position (a fresh
    /// statement inside an arrow body, say) re-enables via the
    /// `parse_stmt` wrapper instead.
    /// Parse `f` with hoisting allowed but into a buffer of its OWN,
    /// handed back next to the result: the caller decides where those
    /// statements go. A `while` / `for` condition (rotation 635) puts
    /// them at the top of the loop body, so they run per iteration —
    /// draining them in front of the statement would run the yield
    /// once, which is why conditions used to reject.
    pub(super) fn with_yield_hoist_captured<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<(T, Vec<Stmt>), String> {
        // The allowed flag is inherited, not forced: a captured
        // position nested inside a refused one (a `&&` rhs inside a
        // destructuring default, say) must stay refused — its
        // statements would land in the refused position's buffer.
        let saved_buf = std::mem::take(&mut self.yield_hoist_buf);
        let r = f(self);
        let captured = std::mem::replace(&mut self.yield_hoist_buf, saved_buf);
        r.map(|v| (v, captured))
    }

    /// A loop whose condition hoisted yields: `while (C) S` becomes
    /// `while (true) { <hoisted>; if (!C') break; S }` — the condition
    /// re-evaluates (and re-yields) per iteration, `continue` reaches
    /// it, `break` still leaves the loop. Answers the rewritten body.
    pub(super) fn loop_body_with_hoisted_cond(
        &mut self,
        hoisted: Vec<Stmt>,
        cond: ExprId,
        body: Stmt,
    ) -> Stmt {
        let not_cond = self.ast.add_expr(Expr::Unary {
            op: crate::ast::UnaryOp::Not,
            expr: cond,
        });
        let mut block = hoisted;
        block.push(Stmt::If {
            cond: not_cond,
            then_branch: Box::new(Stmt::Break(None)),
            else_branch: None,
        });
        block.push(body);
        Stmt::Block(block)
    }

    /// A loop whose STEP (or a do-while's condition) hoisted yields —
    /// positions a `continue` must still reach, which the body-top
    /// rewrite above cannot give them. Rotated instead (rotation 636):
    ///
    /// ```text
    /// do S while (C)         →  for (let f = true; ;) { if (!f) { <hC>; if (!C') break; } f = false; S }
    /// for (I; C; T) S        →  for (I, let f = true; ;) { if (!f) { <hT>; T' } f = false; <hC>; if (!C') break; S }
    /// ```
    ///
    /// The first pass skips the head, every later pass — a `continue`
    /// lands on the loop head, labeled or not — runs the step and
    /// re-evaluates (and re-yields) the condition in source order.
    /// The flag rides the `for` INIT so a label on the source loop
    /// wraps a plain iteration statement and every label reader
    /// (lowering, generator state machine) sees the loop it names.
    /// Answers `(init, body)` for a `Stmt::For` with no cond / step.
    pub(super) fn rotated_loop(
        &mut self,
        init: Option<Box<Stmt>>,
        head: Vec<Stmt>,
        cond: Option<(Vec<Stmt>, ExprId)>,
        body: Stmt,
    ) -> (Option<Box<Stmt>>, Stmt) {
        let id = self.mint_desugar_id();
        let flag = format!("__lrot_{id}");
        let tru = self.ast.add_expr(Expr::Bool(true));
        let flag_decl = Stmt::LetDecl {
            mutable: true,
            name: flag.clone(),
            type_ann: Some("boolean".into()),
            init: tru,
            is_var: false,
        };
        let init = Some(Box::new(match init {
            Some(i) => Stmt::Multi(vec![*i, flag_decl]),
            None => flag_decl,
        }));
        let flag_read = self.ast.add_expr(Expr::Ident(flag.clone()));
        let not_flag = self.ast.add_expr(Expr::Unary {
            op: crate::ast::UnaryOp::Not,
            expr: flag_read,
        });
        let flag_target = self.ast.add_expr(Expr::Ident(flag));
        let fals = self.ast.add_expr(Expr::Bool(false));
        let clear = self.ast.add_expr(Expr::Assign {
            target: flag_target,
            value: fals,
        });
        let mut block = vec![
            Stmt::If {
                cond: not_flag,
                then_branch: Box::new(Stmt::Block(head)),
                else_branch: None,
            },
            Stmt::Expr(clear),
        ];
        if let Some((hoisted, c)) = cond {
            let not_cond = self.ast.add_expr(Expr::Unary {
                op: crate::ast::UnaryOp::Not,
                expr: c,
            });
            block.extend(hoisted);
            block.push(Stmt::If {
                cond: not_cond,
                then_branch: Box::new(Stmt::Break(None)),
                else_branch: None,
            });
        }
        block.push(body);
        (init, Stmt::Block(block))
    }

    pub(super) fn with_yield_hoist_disallowed<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let saved = std::mem::replace(&mut self.yield_hoist_allowed, false);
        let r = f(self);
        self.yield_hoist_allowed = saved;
        r
    }

    /// Run `f` with the formal-parameter-list flag set: §15.1.2 /
    /// §15.8.1 forbid both YieldExpression and AwaitExpression inside
    /// FormalParameters (defaults included), for every function kind
    /// — a generator's or async function's own params too.
    pub(super) fn with_in_formal_params<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let saved = std::mem::replace(&mut self.in_formal_params, true);
        let r = f(self);
        self.in_formal_params = saved;
        r
    }

    /// Run `f` — a FormalParameters parse — with the grammar's
    /// `[Await]` parameter set to this function's own asyncness.
    ///
    /// §15.8.1 gives an async function's FormalParameters `[+Await]`,
    /// so `async function f(await) {}` and `async function f(x =
    /// await) {}` are Syntax Errors while the same spellings under a
    /// plain function in a Script are ordinary names. Every callable
    /// form parses its params BEFORE swapping in the body context, so
    /// without this wrapper the params are judged by the ENCLOSING
    /// bit — which at a program's top level is `false`, and the early
    /// error is silently downgraded to an unknown-identifier warning.
    ///
    /// The companion `in_formal_params` flag is what refuses an
    /// AwaitExpression here; this one refuses the NAME, and only the
    /// two together cover §15.8.1.
    pub(super) fn with_async_params<T>(
        &mut self,
        is_async: bool,
        f: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let saved = std::mem::replace(&mut self.await_reserved, is_async);
        let r = f(self);
        self.await_reserved = saved;
        r
    }

    /// §15.4.1 — SuperProperty is legal anywhere inside a
    /// MethodDefinition, its FormalParameters included: a parameter's
    /// default initializer is evaluated with the method's
    /// [[HomeObject]] in scope, so `{ m(x = super.toString) {} }` is a
    /// legal object literal. Both object-method sites used to raise
    /// the flag only around the BODY — the parameter list parses
    /// before that, so a default landed on the position gate
    /// (`primary_new_super.rs`) and the whole literal was refused.
    /// Twin of [`Self::with_async_params`] next door.
    pub(super) fn with_super_prop<T>(
        &mut self,
        allowed: bool,
        f: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let saved = std::mem::replace(&mut self.super_prop_allowed, allowed);
        let r = f(self);
        self.super_prop_allowed = saved;
        r
    }

    /// §13.15.1 / §13.4.2-5 AssignmentTargetType early errors —
    /// every assignment / update form (`=`, compounds, `++`/`--`)
    /// routes its target through here.
    ///
    /// - A YieldExpression target (`(yield) = v`, `(yield)++`) is a
    ///   SyntaxError at parse time. After hoisting the yield reads
    ///   back as a `__yx_` temp, so a target that IS the temp ident
    ///   can only come from a parenthesized yield in target position
    ///   (`__yx_` is the reserved desugar namespace, same assumption
    ///   `expr_yield_temps` already makes).
    /// - A CallExpression target (`f() = v`, `import(x)++` — the
    ///   ImportCall rewrites to a `Promise.resolve(...)` Call) has
    ///   AssignmentTargetType invalid (rotation 288: the
    ///   statement-position ImportCall dispatch exposed 15 test262
    ///   negatives that previously "passed" on an unrelated parse
    ///   error).
    /// - An `eval` / `arguments` target has AssignmentTargetType
    ///   invalid in strict code (§13.1.3), which is the one arm that
    ///   depends on strictness: the same name READ is legal in the
    ///   strictest module there is, and in sloppy code writing it is
    ///   legal too. Strict by goal is deferred to the prelude gate,
    ///   the shape every strict-refused name already uses.
    pub(super) fn reject_invalid_assignment_target(
        &mut self,
        target: ExprId,
    ) -> Result<(), String> {
        // Classified before anything is recorded: the goal half needs
        // `&mut self`, and the scrutinee's borrow of the arena runs to
        // the end of the match.
        let strict_only = match self.ast.get_expr(target) {
            Expr::Ident(n) if n.starts_with("__yx_") => {
                return Err(format!(
                    "`yield` is not a valid assignment target at {} (ES §13.15.1)",
                    self.at()
                ));
            }
            // §13.3.12.1 — `new.target`'s AssignmentTargetType is
            // invalid. Rejected here at parse time; a later pass gives
            // a plain function's read a parameter to stand in for it.
            Expr::NewTarget => {
                return Err(format!(
                    "`new.target` is not a valid assignment target at {} (ES §13.15.1)",
                    self.at()
                ));
            }
            // §13.15.1 — `this`'s AssignmentTargetType is invalid, so
            // `this = 1` is an EARLY error, not a runtime one. The
            // distinction matters and test262 asserts it
            // (`negative: phase: parse`): `const x = 1; x = 2` has a
            // SIMPLE target and is §6.2.5.6's runtime TypeError,
            // while this one never reaches evaluation at all.
            //
            // It used to be caught further down by accident — the
            // module-level `this` desugars to an immutable binding
            // named `__module_this`, and the checker refused writes
            // to any immutable binding outright. Once that refusal
            // became band B (a warning plus a runtime throw, which is
            // right for a `const`), the three cases asserting the
            // parse phase started failing. The accident is now a
            // rule, in the place the rule belongs.
            Expr::This => {
                return Err(format!(
                    "`this` is not a valid assignment target at {} (ES §13.15.1)",
                    self.at()
                ));
            }
            Expr::Call { .. } | Expr::OptCall { .. } => {
                return Err(format!(
                    "a call expression is not a valid assignment target at {} (ES §13.15.1)",
                    self.at()
                ));
            }
            // A comma expression, or an EFFECTFUL `void <expr>` (the
            // fold keeps the Sequence shape for those) — invalid
            // AssignmentTargetType either way.
            Expr::Sequence { .. } => {
                return Err(format!(
                    "expression is not a valid assignment target at {} (ES §13.15.1)",
                    self.at()
                ));
            }
            // The folded `void <literal>` — the plain `undefined`
            // ident everywhere EXCEPT as an assignment target, where
            // the erased `void` still decides (§13.15.1).
            Expr::Ident(n) if n == "undefined" && self.invalid_target_ids.contains(&target.0) => {
                return Err(format!(
                    "`void` expression is not a valid assignment target at {} (ES §13.15.1)",
                    self.at()
                ));
            }
            Expr::ObjectLit { .. } | Expr::Array(_)
                if self.invalid_target_ids.contains(&target.0) =>
            {
                return Err(format!(
                    "a parenthesized literal is not a valid assignment target at {} (ES §13.15.1)",
                    self.at()
                ));
            }
            Expr::Ident(n) if n == "eval" || n == "arguments" => n.clone(),
            _ => return Ok(()),
        };
        if self.in_strict_fn {
            return Err(format!(
                "`{strict_only}` is not a valid assignment target in strict code at {} (ES §13.1.3)",
                self.at()
            ));
        }
        self.record_strict_goal_site(&strict_only);
        Ok(())
    }

    /// §13.4.1 early error — an UpdateExpression's operand must have
    /// simple AssignmentTargetType: an identifier reference or a
    /// property access (`x`, `o.f`, `a[i]`, parenthesized or not). A
    /// literal, a call, an operator expression, an optional chain
    /// (§13.4 forbids `a?.b++`) is a SyntaxError at parse time. The
    /// prefix form desugared to `x = x + 1` until rotation 626 and
    /// took its reject from the compound clone's own refusal; both
    /// forms are one node now and ask this once, after the
    /// assignment-target check the two share.
    pub(super) fn reject_invalid_update_target(&self, target: ExprId) -> Result<(), String> {
        match self.ast.get_expr(target) {
            Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. } => Ok(()),
            _ => Err(format!(
                "expression is not a valid update target at {} (ES §13.4.1)",
                self.at()
            )),
        }
    }

    /// Drain wrapper around the statement dispatcher — see module doc.
    ///
    /// Also drains `synth_classes_local` (393-01): a class expression
    /// minted while THIS statement parsed lands right in front of it,
    /// so one written in a nested scope stays a nested ClassDecl and
    /// the nested-class machinery gets to decide its fate. A watermark
    /// rather than a full drain because an outer statement's
    /// condition may already have minted (`if ((class {…}).f) { … }`
    /// — the body statements must not adopt the condition's class).
    /// At depth 0 the statement belongs to `parse_program`, whose own
    /// splice keeps top-level behavior byte-identical — hand over.
    pub(super) fn parse_stmt(&mut self) -> Result<Stmt, String> {
        let outer_buf = std::mem::take(&mut self.yield_hoist_buf);
        let synth_mark = self.synth_classes_local.len();
        self.stmt_depth += 1;
        // A fresh statement is an unconditional evaluation position
        // even when the ENCLOSING expression was conditional (an
        // arrow body inside a ternary): re-allow, restore after. The
        // formal-params flag clears for the same reason — a function
        // BODY nested inside a parameter default is its own context.
        let saved_allowed = std::mem::replace(&mut self.yield_hoist_allowed, true);
        let saved_params = std::mem::replace(&mut self.in_formal_params, false);
        let result = self.parse_stmt_dispatch();
        self.stmt_depth -= 1;
        self.in_formal_params = saved_params;
        self.yield_hoist_allowed = saved_allowed;
        let my_buf = std::mem::replace(&mut self.yield_hoist_buf, outer_buf);
        let stmt = result?;
        let mut synths = self.synth_classes_local.split_off(synth_mark);
        if self.stmt_depth == 0 {
            self.synth_classes.append(&mut synths);
        }
        if my_buf.is_empty() && synths.is_empty() {
            return Ok(stmt);
        }
        if !my_buf.is_empty() {
            check_hoist_eval_order(self, &stmt)?;
        }
        let mut v = synths;
        v.extend(my_buf);
        v.push(stmt);
        Ok(Stmt::Multi(v))
    }

    /// Recover the `YieldInto` stmts a destructuring DEFAULT's hoisted
    /// yields left in the buffer, so the caller can re-emit them
    /// inside the undefined-guard the spec puts the default under
    /// (§13.15.5.3 step 4 evaluates Initializer only when the slot
    /// answered undefined — a hoisted yield ran it unconditionally).
    ///
    /// `Ok(None)`: the default reads no yield temp — take the plain
    /// ternary lane. `Ok(Some(stmts))`: every temp's `YieldInto` was
    /// found and removed from the buffer, in buffer (= source) order.
    /// `Err`: some temp has no plain `YieldInto` in the buffer — a
    /// `yield*` default hoists a multi-stmt drive loop this recovery
    /// does not model, so the position keeps its loud reject.
    pub(super) fn recover_yield_temps(
        &mut self,
        default: ExprId,
    ) -> Result<Option<Vec<Stmt>>, String> {
        let temps = super::yield_hoist_events::expr_yield_temps(&self.ast, default);
        if temps.is_empty() {
            return Ok(None);
        }
        let mut recovered = Vec::new();
        self.yield_hoist_buf.retain(|s| match s {
            Stmt::YieldInto { var, .. } if temps.contains(var) => {
                recovered.push(s.clone());
                false
            }
            _ => true,
        });
        if recovered.len() != temps.len() {
            return Err(format!(
                "not yet supported: `yield` in a destructuring-assignment \
                 default (conditional position) at {}",
                self.at()
            ));
        }
        // RFC 20260820-dstr-deferred-close — a recovered yield means
        // the pattern's evaluation can suspend; the expansion wraps
        // its element statements for the deferred IteratorClose.
        self.dstra_saw_yield = true;
        Ok(Some(recovered))
    }
}

use super::yield_hoist_events::check_hoist_eval_order;
