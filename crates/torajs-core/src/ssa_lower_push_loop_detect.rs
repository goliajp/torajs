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
