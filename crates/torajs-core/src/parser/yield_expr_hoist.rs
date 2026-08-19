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
                 (loop condition, short-circuit rhs, ternary branch, optional call, \
                 default value) at {}",
                self.at()
            ));
        }
        self.pos += 1; // consume `yield`
        if matches!(self.peek(), Token::Star) {
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

    /// §13.15.1 / §13.4.2-5 AssignmentTargetType early errors —
    /// every assignment / update form (`=`, compounds, `++`/`--`)
    /// routes its target through here.
    ///
    /// - A YieldExpression target (`(yield) = v`, `(yield)++`) is a
    ///   SyntaxError at parse time. After hoisting the yield reads
    ///   back as a `__yx_` temp, so a target that IS the temp ident
    ///   can only come from a parenthesized yield in target position
    ///   (`__yx_` is the reserved desugar namespace, same assumption
    ///   `expr_reads_yield_temp` already makes).
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
            Expr::Ident(n) if n == "undefined" && self.void_folds.contains(&target.0) => {
                return Err(format!(
                    "`void` expression is not a valid assignment target at {} (ES §13.15.1)",
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
}

use super::yield_hoist_events::check_hoist_eval_order;
pub(super) use super::yield_hoist_events::expr_reads_yield_temp;
