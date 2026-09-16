//! §13.15.5.5 IteratorDestructuringAssignmentEvaluation in spec order
//! — the per-element stepping shape of the destructuring-assignment
//! expansion, sibling to the whole-pattern walk in `dstr_assign.rs`
//! and to the rest half in `dstr_assign_rest.rs`.
//!
//! The spec interleaves three things per element:
//! `lref(i) → step(i) → put(i)`, with `put(i)` before `lref(i+1)`.
//! The walk the plain path emits merges every step into one block up
//! front, which is equivalent only while no target reference is
//! observable to the iterator. A Member / Index target breaks that —
//! `[ {}[f()] ] = it` must run `f()` before `next()` is even looked
//! up, and `[ a[f()], b[g()] ] = it` must interleave `f, next, g,
//! next`. So a pattern carrying one takes this shape instead: the
//! walk stops at a prefix of ZERO (GetIterator and park, no steps),
//! and each element steps the park itself through
//! `__torajs_dstr_step_one`.
//!
//! This is the same machine 刀 D built for the rest element, applied
//! one element earlier. The close obligations ride along unchanged:
//! the pattern sits inside `wrap_deferred_close`, so an abrupt
//! completion with the iterator still live closes it on the way out,
//! while a step that threw or reported done has already cleared the
//! park (§7.4.8) and closes nothing.

use super::*;

impl<'a> Parser<'a> {
    /// Does this assignment pattern need the stepping shape? Only a
    /// Member / Index element target has an observable reference to
    /// order: §13.15.5.5 evaluates no reference at all for a nested
    /// pattern, and a bare identifier's ResolveBinding is not
    /// observable (a strict unresolvable throws at PutValue, after
    /// the step either way).
    ///
    /// A rest element answers false here — 刀 D already evaluates ITS
    /// target before draining. A pattern with both takes both shapes:
    /// the prefix steps element by element and the rest drains from
    /// whatever park they leave.
    pub(super) fn detect_stepwise(&self, target: ExprId) -> bool {
        let Expr::Array(elems) = self.ast.get_expr(target) else {
            return false;
        };
        elems.iter().any(|&e| {
            let t = match self.ast.get_expr(e) {
                Expr::Spread { .. } | Expr::Elision => return false,
                Expr::Assign { target, .. } => *target,
                _ => e,
            };
            matches!(
                self.ast.get_expr(t),
                Expr::Member { .. } | Expr::Index { .. }
            )
        })
    }

    /// Emit the elements of a stepwise pattern. One element is
    /// `lref hoist → step → PutValue`, in that order, which is the
    /// whole point of this file.
    pub(super) fn emit_dstr_assign_array_stepwise(
        &mut self,
        elems: &[ExprId],
        id: u32,
        out: &mut Vec<Stmt>,
    ) -> Result<(), String> {
        for (i, &el) in elems.iter().enumerate() {
            match self.ast.get_expr(el).clone() {
                // An elision binds nothing but still advances the
                // iterator — the step's value is simply dropped.
                Expr::Elision => {
                    let step = self.dstra_step_expr(id);
                    out.push(Stmt::Expr(step));
                }
                Expr::Spread { expr } => {
                    if i + 1 != elems.len() {
                        return Err(format!(
                            "rest element must be last in a destructuring pattern at {}",
                            self.at()
                        ));
                    }
                    self.emit_dstr_rest_deferred(expr, id, out)?;
                }
                Expr::Assign {
                    target,
                    value: default,
                } => {
                    // §8.4.5 NamedEvaluation, as the plain path does.
                    if let Expr::Ident(b) = self.ast.get_expr(target) {
                        let b = b.clone();
                        self.record_dstr_default_name(default, &b);
                    }
                    let rebuilt = self.hoist_elem_lref(target, out)?;
                    let v = self.bind_step_value(id, out);
                    // A yield in the Initializer is CONDITIONAL — it
                    // runs only when the step answered undefined —
                    // so it needs the statement-level guard rather
                    // than the expression ternary.
                    if let Some(recovered) = self.recover_yield_temps(default)? {
                        self.emit_conditional_default(rebuilt, v, default, recovered, out)?;
                    } else {
                        let val = self.dstra_wrap_default(v, Some(default));
                        self.emit_dstr_assign_slot(rebuilt, val, out)?;
                    }
                }
                _ => {
                    let rebuilt = self.hoist_elem_lref(el, out)?;
                    let v = self.bind_step_value(id, out);
                    self.emit_dstr_assign_slot(rebuilt, v, out)?;
                }
            }
        }
        Ok(())
    }

    /// The element target's reference, evaluated BEFORE the step. A
    /// nested pattern has no reference to evaluate (§13.15.5.5 names
    /// only DestructuringAssignmentTarget), and its leaves recover
    /// their own yields when the slot recursion reaches them.
    fn hoist_elem_lref(&mut self, target: ExprId, out: &mut Vec<Stmt>) -> Result<ExprId, String> {
        if matches!(
            self.ast.get_expr(target),
            Expr::Array(_) | Expr::ObjectLit { .. }
        ) {
            return Ok(target);
        }
        if let Some(recovered) = self.recover_yield_temps(target)? {
            out.extend(recovered);
        }
        self.hoist_target_lref(target, out)
    }

    /// `let __dstra_v_<k>: any = <step>;` — the element's value in a
    /// temp, so the default guard and the PutValue can both read it
    /// without stepping twice. Pinned `any`: the generator lift must
    /// not sniff a narrower field type for a slot holding whatever
    /// the iterator yields.
    fn bind_step_value(&mut self, id: u32, out: &mut Vec<Stmt>) -> ExprId {
        let k = self.mint_desugar_id();
        let name = format!("__dstra_v_{k}");
        let step = self.dstra_step_expr(id);
        out.push(Stmt::LetDecl {
            mutable: false,
            name: name.clone(),
            type_ann: Some("any".into()),
            init: step,
            is_var: false,
        });
        self.ast.add_expr(Expr::Ident(name))
    }

    /// `__torajs_dstr_step_one(__dstra_it_<id>, __dstra_raw_<id>)` —
    /// the park slot goes in by NAME because the lowering writes it
    /// back (the next park), which a value argument could not carry.
    fn dstra_step_expr(&mut self, id: u32) -> ExprId {
        let callee = self
            .ast
            .add_expr(Expr::Ident("__torajs_dstr_step_one".into()));
        let it = self.ast.add_expr(Expr::Ident(format!("__dstra_it_{id}")));
        let raw = self.ast.add_expr(Expr::Ident(format!("__dstra_raw_{id}")));
        self.ast.add_expr(Expr::Call {
            callee,
            args: vec![it, raw],
        })
    }
}
