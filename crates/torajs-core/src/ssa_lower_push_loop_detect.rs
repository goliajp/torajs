//! Push-loop pattern detector — pure AST pre-analysis.
//!
//! Recognises the canonical "fill loop"
//!
//! ```ignore
//!   for (let i = 0; i < N; i = i + 1) {
//!     xs.push(_)                  // OR a block/multi of pure xs.push(_) calls
//!   }
//! ```
//!
//! and returns `Some((bound_eid, [xs_name, ...]))` so the caller can
//! emit one `arr_reserve(bound, ...)` per detected array up front and
//! then route each per-iter push through `arr_push_unchecked`. The
//! detector is conservative on the false-positive side: anything not
//! matching the exact shape returns `None` and the regular cap-checked
//! push path runs unchanged. False negatives stay safe (merely
//! slower).
//!
//! Lives in its own file (not collocated with the other AST visitors
//! in `ssa_lower.rs`) so `ssa_lower.rs` stays on the known-debt
//! "only-shrinks" trajectory; mirrors the placement chosen for the
//! 11-A1 deque-escape visitor and 11-A2-a obj-escape visitor.

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::ssa_lower::LowerCtx;

/// v0.6+1 perf checkpoint — see module-level doc for the pattern this
/// recognises.
pub(crate) fn detect_push_loop_arrays(
    ast: &Ast,
    init: Option<&Stmt>,
    cond: Option<ExprId>,
    step: Option<ExprId>,
    body: &Stmt,
) -> Option<(ExprId, Vec<String>)> {
    /* init: `let i = 0` (literal 0; const 0 is enough — anything
     * else means the loop isn't a simple 0..N walk). */
    let i_name = match init? {
        Stmt::LetDecl {
            name,
            init: init_eid,
            ..
        } => match ast.get_expr(*init_eid) {
            Expr::Number(n) if *n == 0.0 => name.clone(),
            _ => return None,
        },
        _ => return None,
    };
    /* cond: `i < bound`. Capture bound expression. */
    let bound_eid = match ast.get_expr(cond?) {
        Expr::BinOp {
            op: crate::ast::BinOp::Lt,
            left,
            right,
        } => match ast.get_expr(*left) {
            Expr::Ident(n) if n == &i_name => *right,
            _ => return None,
        },
        _ => return None,
    };
    /* step: `i = i + 1` shape (parser desugars i++ / i+=1 to this). */
    let step_eid = step?;
    match ast.get_expr(step_eid) {
        Expr::Assign { target, value } => {
            let target_is_i = matches!(ast.get_expr(*target), Expr::Ident(n) if n == &i_name);
            let value_is_i_plus_1 = matches!(
                ast.get_expr(*value),
                Expr::BinOp { op: crate::ast::BinOp::Add, left, right }
                    if matches!(ast.get_expr(*left), Expr::Ident(n) if n == &i_name)
                        && matches!(ast.get_expr(*right), Expr::Number(v) if *v == 1.0)
            );
            if !(target_is_i && value_is_i_plus_1) {
                return None;
            }
        }
        _ => return None,
    }
    /* body: must be Stmt::Expr(push) or Stmt::Block / Multi of
     * push-only stmts (no conditionals, no other method calls).
     * Single-array OR multi-array both work — we collect every
     * `xs.push(_)` target name. */
    let mut names: Vec<String> = Vec::new();
    if !collect_push_targets_only(ast, body, &mut names) {
        return None;
    }
    if names.is_empty() {
        return None;
    }
    Some((bound_eid, names))
}

/// True when every `xs.push(arg)` in `s` has an argument that cannot
/// look at the array being filled, and cannot leave the loop by any
/// edge but its normal exit.
///
/// This is what buys the deferred length word (see
/// [`crate::ssa_lower::PreReserveState::defer_len`]). The body shape
/// the detectors above accept says the loop contains nothing but
/// pushes; it says nothing about what is being pushed, and an
/// argument is an arbitrary expression. `xs.push(xs.length)` reads
/// the very word the loop is holding back, and `xs.push(f(i))` hands
/// control to a function that may read the array or throw out of the
/// loop entirely.
///
/// An allowlist, because the safe direction is to answer no: a shape
/// this does not recognise gets the length word written on every
/// append instead, which costs a store rather than an answer.
/// Literals, numeric locals, and arithmetic over them are the whole
/// list — none of those can call, read a heap cell, or throw. Two
/// absences are deliberate. A member or index read is out: `xs.length`
/// is the case that started this, and an out-of-range index read on a
/// non-`number[]` throws. And the operand types are checked, not just
/// the operator: `a + b` over `any` runs `valueOf`, which is user code
/// with the array in scope.
pub(crate) fn push_args_all_inert(ctx: &LowerCtx<'_>, s: &Stmt) -> bool {
    match s {
        Stmt::Expr(eid) => match ctx.ast.get_expr(*eid) {
            Expr::Call { args, .. } => args.iter().all(|a| expr_is_inert(ctx, *a)),
            // The `while` lane's body carries its `i = i + 1` step as
            // the last statement. Writing an inert value into a local
            // cannot look at the array either.
            Expr::Assign { target, value } => {
                matches!(ctx.ast.get_expr(*target), Expr::Ident(_)) && expr_is_inert(ctx, *value)
            }
            _ => false,
        },
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().all(|s| push_args_all_inert(ctx, s))
        }
        _ => false,
    }
}

/// The allowlist itself. See [`push_args_all_inert`].
fn expr_is_inert(ctx: &LowerCtx<'_>, eid: ExprId) -> bool {
    if !matches!(
        ctx.expr_types.get(&eid),
        Some(crate::check::Type::Number) | Some(crate::check::Type::Boolean)
    ) {
        return false;
    }
    match ctx.ast.get_expr(eid) {
        Expr::Number(_) | Expr::Bool(_) | Expr::Ident(_) => true,
        Expr::BinOp { op, left, right } => {
            use crate::ast::BinOp as B;
            matches!(
                op,
                B::Add
                    | B::Sub
                    | B::Mul
                    | B::Div
                    | B::Mod
                    | B::Lt
                    | B::Gt
                    | B::Le
                    | B::Ge
                    | B::BitAnd
                    | B::BitOr
                    | B::BitXor
                    | B::Shl
                    | B::Shr
                    | B::UShr
            ) && expr_is_inert(ctx, *left)
                && expr_is_inert(ctx, *right)
        }
        _ => false,
    }
}

/// Walk `s` and collect ident names of arrays that are the receiver
/// of a `xs.push(_)` call. Returns `false` if any non-push stmt is
/// found (caller bails). Allows nested Blocks / Multi's so user-
/// formatted bodies parse cleanly.
fn collect_push_targets_only(ast: &Ast, s: &Stmt, out: &mut Vec<String>) -> bool {
    match s {
        Stmt::Expr(eid) => match ast.get_expr(*eid) {
            Expr::Call { callee, args } if args.len() == 1 => {
                let Expr::Member { obj, name } = ast.get_expr(*callee) else {
                    return false;
                };
                if name != "push" {
                    return false;
                }
                let Expr::Ident(xs_name) = ast.get_expr(*obj) else {
                    return false;
                };
                if !out.iter().any(|n| n == xs_name) {
                    out.push(xs_name.clone());
                }
                true
            }
            _ => false,
        },
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().all(|s| collect_push_targets_only(ast, s, out))
        }
        _ => false,
    }
}

/// 12-c-1 — while-shape variant of [`detect_push_loop_arrays`]. Recognises
///
/// ```ignore
///   let i: number = 0;            // immediately-preceding stmt; caller passes counter_name
///   while (i < N) {
///     xs.push(_);                 // any number of xs.push(_) stmts, single- or multi-array
///     // ...
///     i = i + 1;                  // MUST be the last stmt; parser desugars i++ / i+=1 here
///   }
/// ```
///
/// The caller (`Stmt::Block` / `Stmt::Multi` iteration in `ssa_lower`) is
/// responsible for proving `counter_name` is statically 0 at while entry by
/// matching the immediately-preceding stmt's shape via
/// [`let_counter_zero_name`]. Anything else means the loop isn't a simple
/// 0..N walk and the regular cap-checked push path runs unchanged.
///
/// Returns `Some((bound_eid, [xs_name, ...]))` when the shape matches.
/// False negatives stay safe; false positives are guarded against by the
/// caller-side init check + this fn's strict body-shape check.
pub(crate) fn detect_push_loop_arrays_while(
    ast: &Ast,
    counter_name: &str,
    cond: ExprId,
    body: &Stmt,
) -> Option<(ExprId, Vec<String>)> {
    /* cond: `counter < bound`. Capture bound expression. */
    let bound_eid = match ast.get_expr(cond) {
        Expr::BinOp {
            op: crate::ast::BinOp::Lt,
            left,
            right,
        } => match ast.get_expr(*left) {
            Expr::Ident(n) if n == counter_name => *right,
            _ => return None,
        },
        _ => return None,
    };
    /* body: must be Block/Multi where the LAST stmt is the canonical
     * `counter = counter + 1` step and every earlier stmt is a pure
     * xs.push(_) (single- or multi-array; nested Block/Multi allowed
     * via collect_push_targets_only's recursion). */
    let stmts: &[Stmt] = match body {
        Stmt::Block(s) | Stmt::Multi(s) => s.as_slice(),
        _ => return None,
    };
    let (last, init_stmts) = stmts.split_last()?;
    if !is_counter_step_stmt(ast, last, counter_name) {
        return None;
    }
    let mut names: Vec<String> = Vec::new();
    for s in init_stmts {
        if !collect_push_targets_only(ast, s, &mut names) {
            return None;
        }
    }
    if names.is_empty() {
        return None;
    }
    Some((bound_eid, names))
}

/// 12-c-1 — returns Some(counter_name) when `s` is `let counter = 0`
/// (literal `0` init; type-annotation optional). Proves the counter is
/// statically 0 at the immediately-following while entry — caller
/// threads this through [`detect_push_loop_arrays_while`].
pub(crate) fn let_counter_zero_name(ast: &Ast, s: Option<&Stmt>) -> Option<String> {
    let Stmt::LetDecl {
        name,
        init: init_eid,
        ..
    } = s?
    else {
        return None;
    };
    match ast.get_expr(*init_eid) {
        Expr::Number(n) if *n == 0.0 => Some(name.clone()),
        _ => None,
    }
}

fn is_counter_step_stmt(ast: &Ast, s: &Stmt, counter_name: &str) -> bool {
    let Stmt::Expr(eid) = s else {
        return false;
    };
    let Expr::Assign { target, value } = ast.get_expr(*eid) else {
        return false;
    };
    let target_is_counter = matches!(ast.get_expr(*target), Expr::Ident(n) if n == counter_name);
    let value_is_counter_plus_1 = matches!(
        ast.get_expr(*value),
        Expr::BinOp { op: crate::ast::BinOp::Add, left, right }
            if matches!(ast.get_expr(*left), Expr::Ident(n) if n == counter_name)
                && matches!(ast.get_expr(*right), Expr::Number(v) if *v == 1.0)
    );
    target_is_counter && value_is_counter_plus_1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{BinOp as AstBinOp, Expr, Stmt};

    /* Helpers — construct the AST fragments by hand. We bypass the
     * parser so the tests stay sensitive to the detector's exact
     * shape contract; any future parser desugar that breaks the
     * canonical shape will be caught by the call-site, not here. */

    fn mk_ident(ast: &mut Ast, name: &str) -> ExprId {
        ast.add_expr(Expr::Ident(name.to_string()))
    }
    fn mk_number(ast: &mut Ast, n: f64) -> ExprId {
        ast.add_expr(Expr::Number(n))
    }
    fn mk_lt(ast: &mut Ast, left: ExprId, right: ExprId) -> ExprId {
        ast.add_expr(Expr::BinOp {
            op: AstBinOp::Lt,
            left,
            right,
        })
    }
    fn mk_add(ast: &mut Ast, left: ExprId, right: ExprId) -> ExprId {
        ast.add_expr(Expr::BinOp {
            op: AstBinOp::Add,
            left,
            right,
        })
    }
    fn mk_assign(ast: &mut Ast, target: ExprId, value: ExprId) -> ExprId {
        ast.add_expr(Expr::Assign { target, value })
    }
    fn mk_push_call(ast: &mut Ast, receiver: &str, arg: ExprId) -> ExprId {
        let obj = mk_ident(ast, receiver);
        let callee = ast.add_expr(Expr::Member {
            obj,
            name: "push".to_string(),
        });
        ast.add_expr(Expr::Call {
            callee,
            args: vec![arg],
        })
    }

    fn mk_let_zero(ast: &mut Ast, name: &str) -> Stmt {
        let init = mk_number(ast, 0.0);
        Stmt::LetDecl {
            mutable: true,
            name: name.to_string(),
            type_ann: Some("number".to_string()),
            init,
            is_var: false,
        }
    }

    fn mk_step(ast: &mut Ast, counter: &str) -> Stmt {
        let t = mk_ident(ast, counter);
        let l = mk_ident(ast, counter);
        let r = mk_number(ast, 1.0);
        let plus = mk_add(ast, l, r);
        let assign = mk_assign(ast, t, plus);
        Stmt::Expr(assign)
    }

    fn mk_push_stmt(ast: &mut Ast, receiver: &str, arg_name: &str) -> Stmt {
        let arg = mk_ident(ast, arg_name);
        let call = mk_push_call(ast, receiver, arg);
        Stmt::Expr(call)
    }

    #[test]
    fn let_counter_zero_name_matches_literal_zero_init() {
        let mut ast = Ast::default();
        let s = mk_let_zero(&mut ast, "i");
        assert_eq!(let_counter_zero_name(&ast, Some(&s)), Some("i".to_string()));
    }

    #[test]
    fn let_counter_zero_name_rejects_nonzero_init() {
        let mut ast = Ast::default();
        let init = mk_number(&mut ast, 5.0);
        let s = Stmt::LetDecl {
            mutable: true,
            name: "i".to_string(),
            type_ann: Some("number".to_string()),
            init,
            is_var: false,
        };
        assert_eq!(let_counter_zero_name(&ast, Some(&s)), None);
    }

    #[test]
    fn let_counter_zero_name_rejects_non_letdecl() {
        let mut ast = Ast::default();
        let arg = mk_number(&mut ast, 0.0);
        let s = Stmt::Expr(arg);
        assert_eq!(let_counter_zero_name(&ast, Some(&s)), None);
    }

    #[test]
    fn let_counter_zero_name_rejects_none() {
        let ast = Ast::default();
        assert_eq!(let_counter_zero_name(&ast, None), None);
    }

    #[test]
    fn detect_while_matches_canonical_array_sum_shape() {
        /* let xs: number[] = []; let i: number = 0;
         * while (i < 10000000) { xs.push(i); i = i + 1; }
         * — array-sum-1m bench shape. */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 10_000_000.0);
        let cond = mk_lt(&mut ast, i_ref, bound);
        let push = mk_push_stmt(&mut ast, "xs", "i");
        let step = mk_step(&mut ast, "i");
        let body = Stmt::Block(vec![push, step]);

        let result = detect_push_loop_arrays_while(&ast, "i", cond, &body);
        assert!(result.is_some(), "canonical shape must match");
        let (got_bound, names) = result.unwrap();
        assert_eq!(got_bound, bound);
        assert_eq!(names, vec!["xs".to_string()]);
    }

    #[test]
    fn detect_while_multi_array_push() {
        /* while (i < N) { xs.push(i); ys.push(i); i = i + 1; } */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 100.0);
        let cond = mk_lt(&mut ast, i_ref, bound);
        let push_xs = mk_push_stmt(&mut ast, "xs", "i");
        let push_ys = mk_push_stmt(&mut ast, "ys", "i");
        let step = mk_step(&mut ast, "i");
        let body = Stmt::Block(vec![push_xs, push_ys, step]);

        let result = detect_push_loop_arrays_while(&ast, "i", cond, &body);
        let (_, names) = result.expect("multi-array shape must match");
        assert_eq!(names, vec!["xs".to_string(), "ys".to_string()]);
    }

    #[test]
    fn detect_while_rejects_missing_step() {
        /* while (i < N) { xs.push(i); }  ← no step; would loop forever
         * in real code, but the parser still produces this AST. */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 10.0);
        let cond = mk_lt(&mut ast, i_ref, bound);
        let push = mk_push_stmt(&mut ast, "xs", "i");
        let body = Stmt::Block(vec![push]);

        assert!(detect_push_loop_arrays_while(&ast, "i", cond, &body).is_none());
    }

    #[test]
    fn detect_while_rejects_step_not_last() {
        /* while (i < N) { i = i + 1; xs.push(i); } */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 10.0);
        let cond = mk_lt(&mut ast, i_ref, bound);
        let step = mk_step(&mut ast, "i");
        let push = mk_push_stmt(&mut ast, "xs", "i");
        let body = Stmt::Block(vec![step, push]);

        /* When the supposed step is first, it gets routed through
         * collect_push_targets_only as a non-push stmt and rejected;
         * the actual last stmt is a push, which fails is_counter_step. */
        assert!(detect_push_loop_arrays_while(&ast, "i", cond, &body).is_none());
    }

    #[test]
    fn detect_while_rejects_wrong_counter_in_cond() {
        /* while (j < N) { xs.push(i); i = i + 1; }  ← cond uses j, not i */
        let mut ast = Ast::default();
        let j_ref = mk_ident(&mut ast, "j");
        let bound = mk_number(&mut ast, 10.0);
        let cond = mk_lt(&mut ast, j_ref, bound);
        let push = mk_push_stmt(&mut ast, "xs", "i");
        let step = mk_step(&mut ast, "i");
        let body = Stmt::Block(vec![push, step]);

        assert!(detect_push_loop_arrays_while(&ast, "i", cond, &body).is_none());
    }

    #[test]
    fn detect_while_rejects_wrong_op_in_cond() {
        /* while (i <= N) { ... }  ← Le not Lt */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 10.0);
        let cond = ast.add_expr(Expr::BinOp {
            op: AstBinOp::Le,
            left: i_ref,
            right: bound,
        });
        let push = mk_push_stmt(&mut ast, "xs", "i");
        let step = mk_step(&mut ast, "i");
        let body = Stmt::Block(vec![push, step]);

        assert!(detect_push_loop_arrays_while(&ast, "i", cond, &body).is_none());
    }

    #[test]
    fn detect_while_rejects_step_increment_not_one() {
        /* while (i < N) { xs.push(i); i = i + 2; }  ← step+2 */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 10.0);
        let cond = mk_lt(&mut ast, i_ref, bound);
        let push = mk_push_stmt(&mut ast, "xs", "i");
        let t = mk_ident(&mut ast, "i");
        let l = mk_ident(&mut ast, "i");
        let two = mk_number(&mut ast, 2.0);
        let plus = mk_add(&mut ast, l, two);
        let assign = mk_assign(&mut ast, t, plus);
        let bad_step = Stmt::Expr(assign);
        let body = Stmt::Block(vec![push, bad_step]);

        assert!(detect_push_loop_arrays_while(&ast, "i", cond, &body).is_none());
    }

    #[test]
    fn detect_while_rejects_non_push_body_stmt() {
        /* while (i < N) { console.log(i); i = i + 1; }  ← non-push */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 10.0);
        let cond = mk_lt(&mut ast, i_ref, bound);
        let console = mk_ident(&mut ast, "console");
        let log_callee = ast.add_expr(Expr::Member {
            obj: console,
            name: "log".to_string(),
        });
        let arg_i = mk_ident(&mut ast, "i");
        let log_call = ast.add_expr(Expr::Call {
            callee: log_callee,
            args: vec![arg_i],
        });
        let log_stmt = Stmt::Expr(log_call);
        let step = mk_step(&mut ast, "i");
        let body = Stmt::Block(vec![log_stmt, step]);

        assert!(detect_push_loop_arrays_while(&ast, "i", cond, &body).is_none());
    }

    #[test]
    fn detect_while_rejects_non_block_body() {
        /* while (i < N) i = i + 1;  ← single-stmt while body without block */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 10.0);
        let cond = mk_lt(&mut ast, i_ref, bound);
        let body = mk_step(&mut ast, "i");

        assert!(detect_push_loop_arrays_while(&ast, "i", cond, &body).is_none());
    }

    #[test]
    fn detect_while_rejects_push_with_extra_args() {
        /* while (i < N) { xs.push(i, j); i = i + 1; }  ← .push takes 1 arg */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 10.0);
        let cond = mk_lt(&mut ast, i_ref, bound);
        let xs = mk_ident(&mut ast, "xs");
        let callee = ast.add_expr(Expr::Member {
            obj: xs,
            name: "push".to_string(),
        });
        let a1 = mk_ident(&mut ast, "i");
        let a2 = mk_ident(&mut ast, "j");
        let call = ast.add_expr(Expr::Call {
            callee,
            args: vec![a1, a2],
        });
        let push2 = Stmt::Expr(call);
        let step = mk_step(&mut ast, "i");
        let body = Stmt::Block(vec![push2, step]);

        assert!(detect_push_loop_arrays_while(&ast, "i", cond, &body).is_none());
    }

    #[test]
    fn detect_while_accepts_multi_body() {
        /* Body wrapped in Stmt::Multi (synthetic, post-desugar shape). */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 10.0);
        let cond = mk_lt(&mut ast, i_ref, bound);
        let push = mk_push_stmt(&mut ast, "xs", "i");
        let step = mk_step(&mut ast, "i");
        let body = Stmt::Multi(vec![push, step]);

        assert!(detect_push_loop_arrays_while(&ast, "i", cond, &body).is_some());
    }
}
