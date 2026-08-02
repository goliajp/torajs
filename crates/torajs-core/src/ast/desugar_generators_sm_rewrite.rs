//! Yield-detection + break/continue-rewrite helpers for the generator
//! state-machine emitter, split out of `desugar_generators_sm` (labeled
//! break/continue work) to keep that file under the size limit. Both
//! functions are `pub(super)` and consumed only by [`super::GenSm`].

use super::desugar_generators_sm::{DISPATCH_LABEL, RESUME_LOCAL};
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
/// continue __sm;` gotos that re-enter the enclosing `while (true)`
/// state machine at the loop's continue / break target. A bare jump
/// inside a nested yield-free `while` / `for` / `switch` belongs to
/// that construct and stays literal (`bare_owned`), but a LABELED
/// jump resolves through any depth — the goto is a labeled continue,
/// so binding is depth-independent, and ES §13.13 forbids duplicate
/// labels in a nesting chain, so a label resolving to a yield-loop
/// on the stack can't be shadowed by an inner one.
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
    rewrite_jumps(ast, s, loop_stack, frames, false)
}

fn rewrite_jumps(
    ast: &mut Ast,
    s: &mut Stmt,
    loop_stack: &[(usize, usize, Option<String>)],
    frames: &mut Vec<super::desugar_generators_sm_finally::FinallyRetFrame>,
    bare_owned: bool,
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
        Stmt::Block(vec![
            Stmt::Expr(assign),
            Stmt::Continue(Some(DISPATCH_LABEL.into())),
        ])
    }
    /// D4 — the routed variant: goto a placeholder recorded on the
    /// innermost finally frame's (kind, label)-keyed list; the
    /// frame's F copy entry is patched in afterwards. Inline mirror
    /// of [`super::GenSm::build_finally_jump_stmts`].
    fn make_routed_goto(
        ast: &mut Ast,
        frames: &mut [super::desugar_generators_sm_finally::FinallyRetFrame],
        want_break: bool,
        label: Option<String>,
    ) -> Stmt {
        let st = ast.add_expr(Expr::Ident(RESUME_LOCAL.into()));
        let placeholder = ast.add_expr(Expr::Number(0.0));
        let assign = ast.add_expr(Expr::Assign {
            target: st,
            value: placeholder,
        });
        let frame = frames.last_mut().expect("caller checked");
        frame.jump_gotos.push(((want_break, label), placeholder));
        Stmt::Block(vec![
            Stmt::Expr(assign),
            Stmt::Continue(Some(DISPATCH_LABEL.into())),
        ])
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
            if l.is_none() && bare_owned {
                return;
            }
            if let Some(cont) = resolve(l, false) {
                *s = if escapes(cont, frames) {
                    make_routed_goto(ast, frames, false, l.clone())
                } else {
                    make_goto(ast, cont)
                };
            }
        }
        Stmt::Break(l) => {
            if l.is_none() && bare_owned {
                return;
            }
            if let Some(brk) = resolve(l, true) {
                *s = if escapes(brk, frames) {
                    make_routed_goto(ast, frames, true, l.clone())
                } else {
                    make_goto(ast, brk)
                };
            }
        }
        // Inner loops own their bare break/continue; labeled jumps
        // still resolve through them (the goto is depth-independent).
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
            rewrite_jumps(ast, body, loop_stack, frames, true);
        }
        // Switch swallows a bare `break` (it targets the switch); a
        // bare `continue` inside it targets the enclosing loop, but
        // that pre-existing face stays untouched (bare_owned covers
        // both). Labeled jumps resolve through.
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                for x in &mut c.body {
                    rewrite_jumps(ast, x, loop_stack, frames, true);
                }
            }
            if let Some(ds) = default {
                for x in ds {
                    rewrite_jumps(ast, x, loop_stack, frames, true);
                }
            }
        }
        // An inline try owns no jumps — descend transparently.
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for x in body.iter_mut().chain(catch_body.iter_mut()) {
                rewrite_jumps(ast, x, loop_stack, frames, bare_owned);
            }
            if let Some(fs) = finally_body {
                for x in fs {
                    rewrite_jumps(ast, x, loop_stack, frames, bare_owned);
                }
            }
        }
        // A labeled statement's own label can't collide with a
        // yield-loop label (§13.13 duplicate-label early error), so
        // jumps naming an OUTER yield-loop still resolve; its own
        // jumps resolve to nothing on the stack and stay literal.
        Stmt::Labeled { body, .. } => {
            rewrite_jumps(ast, body, loop_stack, frames, bare_owned);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_jumps(ast, then_branch, loop_stack, frames, bare_owned);
            if let Some(eb) = else_branch.as_deref_mut() {
                rewrite_jumps(ast, eb, loop_stack, frames, bare_owned);
            }
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                rewrite_jumps(ast, s, loop_stack, frames, bare_owned);
            }
        }
        _ => {}
    }
}
