//! RFC 20260801 — the `...arguments` spread-expansion family for
//! [`super::arguments_object_rewrite`], split to a sibling when
//! knife 2's walker-mirror arms pushed the rewriter past the
//! 500-line file limit. `rewrite_call_arm` / `rewrite_array_arm`
//! swap or inline-expand an `arguments` spread source; they recurse
//! back into `rewrite_arguments_in_expr` for every other child.

use super::arguments_object::ArgcMode;
use super::arguments_object_rewrite::rewrite_arguments_in_expr;
use super::{Ast, Expr, ExprId};

/// Length-write knife — a LiveLength body's `...arguments` spread
/// must read the LIVE materialized array (a length write may have
/// grown or truncated it); inline-expanding the static prefix would
/// answer the stale pre-write shape. The argv face already spreads
/// the array for its own reason (beyond-declared values), and the
/// unmapped face for element-write visibility (`arguments[0] = 2`
/// must show in a later spread — params never alias). Everything
/// else (Real / FoldArity — no materialized array) expands the
/// declared params inline: declared == actual on those paths.
fn spread_rides_array(argc_mode: ArgcMode, is_argv_fn: bool) -> bool {
    is_argv_fn || matches!(argc_mode, ArgcMode::LiveLength(_) | ArgcMode::Unmapped(_))
}

/// `f(...arguments)` — argv-face bodies swap the spread source to the
/// materialized `__torajs_arguments` array (apply_spread_args expands
/// it downstream against fixed-arity callees; rest-callees take it
/// directly); named-fn / declared-pair bodies expand the spread inline
/// as `f(p0, p1, ...)` — declared == actual there. Handles arbitrary
/// mix of regular args and the spread.
pub(super) fn rewrite_call_arm(
    ast: &mut Ast,
    eid: ExprId,
    callee: ExprId,
    args: Vec<ExprId>,
    params: &[String],
    argc_mode: ArgcMode,
    is_argv_fn: bool,
) -> ExprId {
    let c = rewrite_arguments_in_expr(ast, callee, params, argc_mode, is_argv_fn);
    let mut new_args: Vec<ExprId> = Vec::with_capacity(args.len());
    for a in &args {
        if let Expr::Spread { expr } = ast.get_expr(*a)
            && let Expr::Ident(n) = ast.get_expr(*expr)
            && n == "arguments"
        {
            if spread_rides_array(argc_mode, is_argv_fn) {
                let src = ast.add_expr(Expr::Ident("__torajs_arguments".into()));
                new_args.push(ast.add_expr(Expr::Spread { expr: src }));
            } else {
                for p in params {
                    new_args.push(ast.add_expr(Expr::Ident(p.clone())));
                }
            }
            continue;
        }
        new_args.push(rewrite_arguments_in_expr(
            ast, *a, params, argc_mode, is_argv_fn,
        ));
    }
    if c == callee && new_args == args {
        return eid;
    }
    let new_id = ast.add_expr(Expr::Call {
        callee: c,
        args: new_args,
    });
    // A genuinely-rewritten call keeps its speculative class-method
    // demotion candidacy (cm_demote.rs) — the map is keyed by the
    // Call node the checker will visit.
    if let Some(&alt) = ast.speculative_cm_rewrites.get(&eid) {
        ast.speculative_cm_rewrites.insert(new_id, alt);
    }
    new_id
}

/// `[...arguments]` — argv-face bodies swap the source to
/// `[...__torajs_arguments]` (the existing Arr<Any> literal-spread
/// lane); named-fn / declared-pair bodies expand inline. Same shape as
/// the Call arm above. Mixed elems (regular + spread) supported by
/// interleaving.
pub(super) fn rewrite_array_arm(
    ast: &mut Ast,
    eid: ExprId,
    elems: Vec<ExprId>,
    params: &[String],
    argc_mode: ArgcMode,
    is_argv_fn: bool,
) -> ExprId {
    let mut new_elems: Vec<ExprId> = Vec::with_capacity(elems.len());
    for e in &elems {
        if let Expr::Spread { expr } = ast.get_expr(*e)
            && let Expr::Ident(n) = ast.get_expr(*expr)
            && n == "arguments"
        {
            if spread_rides_array(argc_mode, is_argv_fn) {
                let src = ast.add_expr(Expr::Ident("__torajs_arguments".into()));
                new_elems.push(ast.add_expr(Expr::Spread { expr: src }));
            } else {
                for p in params {
                    new_elems.push(ast.add_expr(Expr::Ident(p.clone())));
                }
            }
            continue;
        }
        new_elems.push(rewrite_arguments_in_expr(
            ast, *e, params, argc_mode, is_argv_fn,
        ));
    }
    if new_elems == elems {
        return eid;
    }
    ast.add_expr(Expr::Array(new_elems))
}
