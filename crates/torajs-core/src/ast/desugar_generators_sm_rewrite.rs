//! Yield-detection + break/continue-rewrite helpers for the generator
//! state-machine emitter, split out of `desugar_generators_sm` (labeled
//! break/continue work) to keep that file under the size limit. Both
//! functions are `pub(super)` and consumed only by [`super::GenSm`].

use super::desugar_generators_sm::RESUME_LOCAL;
use super::{Ast, Expr, Stmt};

/// Returns true if `s` (or any nested stmt) contains a `yield`. Used
/// by `GenSm` to decide whether a control-flow construct must be
/// expanded into separate state arms (yields present) or can be
/// emitted inline as a regular Stmt::If / While / For.
pub(super) fn stmt_contains_yield(s: &Stmt) -> bool {
    match s {
        Stmt::Yield(_) | Stmt::YieldInto { .. } => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmt_contains_yield(then_branch)
                || else_branch.as_deref().is_some_and(stmt_contains_yield)
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => stmt_contains_yield(body),
        Stmt::For { init, body, .. } => {
            init.as_deref().is_some_and(stmt_contains_yield) || stmt_contains_yield(body)
        }
        Stmt::Labeled { body, .. } => stmt_contains_yield(body),
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.iter().any(stmt_contains_yield),
        Stmt::Switch { cases, default, .. } => {
            cases.iter().any(|c| c.body.iter().any(stmt_contains_yield))
                || default
                    .as_ref()
                    .is_some_and(|d| d.iter().any(stmt_contains_yield))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(stmt_contains_yield)
                || catch_body.iter().any(stmt_contains_yield)
                || finally_body
                    .as_ref()
                    .is_some_and(|f| f.iter().any(stmt_contains_yield))
        }
        _ => false,
    }
}

/// Rewrite `continue;` / `break;` inside `s` into `state = <target>;
/// continue;` gotos that re-enter the enclosing `while (true)` state
/// machine at the loop's continue / break target. Stops at inner loop
/// boundaries — break/continue inside a nested yield-free
/// `while` / `for` belong to that inner loop and stay literal.
///
/// `loop_stack` is the enclosing yield-bearing loops (innermost last),
/// each `(continue_state, break_state, label)`. A bare `break`/
/// `continue` rewrites to the innermost loop's target; a
/// `break label`/`continue label` resolves to the matching enclosing
/// yield-loop's state at any depth. A labeled jump naming no yield-loop
/// on the stack is left literal (a yield-free inner loop owns it, or
/// ssa_lower resolves it).
pub(super) fn rewrite_break_continue_for_outer(
    ast: &mut Ast,
    s: &mut Stmt,
    loop_stack: &[(usize, usize, Option<String>)],
    frames: &mut Vec<super::desugar_generators_sm_finally::FinallyRetFrame>,
) {
    /// A rewritten `break` / `continue` is a goto, so it moves the local
    /// resume cursor — same as [`super::GenSm::emit_goto`], and for the
    /// same reason (see [`RESUME_LOCAL`]). Writing `this.__state` here
    /// while the dispatch reads the local would re-enter the SAME arm on
    /// every turn of the `while (true)`: an infinite loop in any
    /// generator whose yield-bearing loop breaks or continues.
    fn make_goto(ast: &mut Ast, target: usize) -> Stmt {
        let st = ast.add_expr(Expr::Ident(RESUME_LOCAL.into()));
        let lit = ast.add_expr(Expr::Number(target as f64));
        let assign = ast.add_expr(Expr::Assign {
            target: st,
            value: lit,
        });
        Stmt::Block(vec![Stmt::Expr(assign), Stmt::Continue(None)])
    }
    /// D4 — the routed variant: goto a placeholder recorded on the
    /// innermost finally frame's per-kind list; the frame's F copy
    /// entry is patched in afterwards. Inline mirror of
    /// [`super::GenSm::build_finally_jump_stmts`].
    fn make_routed_goto(
        ast: &mut Ast,
        frames: &mut [super::desugar_generators_sm_finally::FinallyRetFrame],
        want_break: bool,
    ) -> Stmt {
        let st = ast.add_expr(Expr::Ident(RESUME_LOCAL.into()));
        let placeholder = ast.add_expr(Expr::Number(0.0));
        let assign = ast.add_expr(Expr::Assign {
            target: st,
            value: placeholder,
        });
        let frame = frames.last_mut().expect("caller checked");
        if want_break {
            frame.brk_gotos.push(placeholder);
        } else {
            frame.cont_gotos.push(placeholder);
        }
        Stmt::Block(vec![Stmt::Expr(assign), Stmt::Continue(None)])
    }
    // Resolve a bare / labeled jump to a yield-loop state on the stack.
    // Bare → innermost; labeled → the matching enclosing yield-loop.
    let resolve = |l: &Option<String>, want_break: bool| -> Option<usize> {
        let pick = |&(cont, brk, _): &(usize, usize, Option<String>)| {
            if want_break { brk } else { cont }
        };
        match l {
            None => loop_stack.last().map(pick),
            Some(name) => loop_stack
                .iter()
                .rev()
                .find(|(_, _, lbl)| lbl.as_deref() == Some(name.as_str()))
                .map(pick),
        }
    };
    // D4 route test — bare jump whose target loop was entered before
    // the innermost enclosing try/finally (monotonic state alloc ⇒
    // smaller state = outside the try) must run F on the way out.
    let escapes =
        |target: usize, frames: &[super::desugar_generators_sm_finally::FinallyRetFrame]| {
            frames.last().is_some_and(|f| target < f.try_entry)
        };
    match s {
        Stmt::Continue(l) => {
            if let Some(cont) = resolve(l, false) {
                *s = if l.is_none() && escapes(cont, frames) {
                    make_routed_goto(ast, frames, false)
                } else {
                    make_goto(ast, cont)
                };
            }
        }
        Stmt::Break(l) => {
            if let Some(brk) = resolve(l, true) {
                *s = if l.is_none() && escapes(brk, frames) {
                    make_routed_goto(ast, frames, true)
                } else {
                    make_goto(ast, brk)
                };
            }
        }
        // Inner loops own their bare break/continue — don't descend.
        Stmt::While { .. } | Stmt::DoWhile { .. } | Stmt::For { .. } => {}
        // Switch swallows `break` (it targets the switch). `continue`
        // inside a switch belongs to the enclosing loop, but yields
        // inside switch aren't in J.2.b scope so we don't touch this.
        Stmt::Switch { .. } => {}
        Stmt::Try { .. } => {}
        // A labeled statement wraps its own loop/block that owns its
        // jumps — leave literal (ssa_lower resolves it).
        Stmt::Labeled { .. } => {}
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_break_continue_for_outer(ast, then_branch, loop_stack, frames);
            if let Some(eb) = else_branch.as_deref_mut() {
                rewrite_break_continue_for_outer(ast, eb, loop_stack, frames);
            }
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                rewrite_break_continue_for_outer(ast, s, loop_stack, frames);
            }
        }
        _ => {}
    }
}
