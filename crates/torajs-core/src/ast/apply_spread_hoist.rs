//! Non-Ident spread-source hoist (chunk 689, spread longtail #5).
//!
//! Every spread walk (fixed-arity direct expansion, Math.min/max,
//! xs.push) needs an Ident source — the expansion re-reads it per
//! slot. A call / member source (`sum2(...mk())`, `f(...o.arr)`)
//! hoists into a compiler temp ahead of the statement:
//!
//! ```text
//! console.log(sum2(...mk()));
//!   ⇒ Multi[ let __sps_N = [...mk()];
//!            console.log(sum2(...__sps_N)); ]
//! ```
//!
//! `Stmt::Multi` shares the surrounding scope, so a hoist ahead of
//! a `let` / `return` statement stays visible. The hoist moves the
//! source's evaluation ahead of the whole statement, so it only
//! fires when that is observably equivalent: the spread call must
//! sit on the statement's FIRST-evaluation chain (root expr, then
//! arg 0 of each enclosing call whose callee is an effect-free
//! Ident / Ident-member), and its fixed prefix args must be
//! effect-free (literals / Idents). Anything else keeps the
//! checker's existing loud reject. Runs first inside
//! `apply_spread_args`, so the sibling walks see the Ident temp.
//!
//! An Ident source hoists too unless it is syntactically an array
//! (rotation 626, see `apply_spread_evidence`): the walks index the
//! temp, and a generator / Set / string Ident indexed is not
//! spread — the array literal here is what runs §7.4 on it.

use super::apply_spread_evidence::collect_array_evident;
use super::{Ast, Expr, ExprId, Stmt};
use std::collections::HashSet;

pub(super) fn apply_spread_hoist(ast: &mut Ast) {
    let evident = collect_array_evident(ast);
    let mut counter = 0usize;
    let mut stmts = std::mem::take(&mut ast.stmts);
    for s in &mut stmts {
        rewrite_stmt(ast, s, &mut counter, &evident);
    }
    ast.stmts = stmts;
}

fn rewrite_stmt(ast: &mut Ast, s: &mut Stmt, counter: &mut usize, evident: &HashSet<String>) {
    // Recurse into nested bodies first, then try the statement's own
    // root expr (Expr / LetDecl-init / Return positions).
    match s {
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for c in stmts {
                rewrite_stmt(ast, c, counter, evident);
            }
            return;
        }
        Stmt::FnDecl { body, .. } => {
            for c in body {
                rewrite_stmt(ast, c, counter, evident);
            }
            return;
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_stmt(ast, then_branch, counter, evident);
            if let Some(eb) = else_branch {
                rewrite_stmt(ast, eb, counter, evident);
            }
            return;
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            rewrite_stmt(ast, body, counter, evident);
            return;
        }
        Stmt::Labeled { body, .. } => {
            rewrite_stmt(ast, body, counter, evident);
            return;
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                rewrite_stmt(ast, i, counter, evident);
            }
            rewrite_stmt(ast, body, counter, evident);
            return;
        }
        Stmt::ForOf { body, .. } | Stmt::ForOfSplitIter { body, .. } => {
            rewrite_stmt(ast, body, counter, evident);
            return;
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for c in body.iter_mut().chain(catch_body.iter_mut()) {
                rewrite_stmt(ast, c, counter, evident);
            }
            if let Some(fin) = finally_body {
                for c in fin {
                    rewrite_stmt(ast, c, counter, evident);
                }
            }
            return;
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut().flat_map(|c| c.body.iter_mut()) {
                rewrite_stmt(ast, c, counter, evident);
            }
            if let Some(d) = default {
                for c in d {
                    rewrite_stmt(ast, c, counter, evident);
                }
            }
            return;
        }
        _ => {}
    }
    let root = match s {
        Stmt::Expr(eid) => *eid,
        Stmt::LetDecl { init, .. } => *init,
        Stmt::Return(Some(eid)) => *eid,
        _ => return,
    };
    let Some((spread_eid, src_eid)) = find_hoist_target(ast, root, evident) else {
        return;
    };
    let temp = format!("__sps_{}", *counter);
    *counter += 1;
    let temp_ref = ast.add_expr(Expr::Ident(temp.clone()));
    ast.exprs[spread_eid.0 as usize] = Expr::Spread { expr: temp_ref };
    // The temp holds `[...src]`, not `src`: the expansions this hoist
    // feeds read the temp by INDEX, which is only the spread's
    // meaning for an array. A generator or any other iterable source
    // was hoisted as itself and then indexed — its body never ran,
    // its throw never surfaced, and every slot read undefined
    // (`f(...gen())` on `function f(a, b)`, rotation 625). The array
    // literal spread runs §7.4 once, here, in the source's own
    // position; an array source pays one copy for it.
    let src_spread = ast.add_expr(Expr::Spread { expr: src_eid });
    let materialized = ast.add_expr(Expr::Array(vec![src_spread]));
    let hoist = Stmt::LetDecl {
        mutable: false,
        name: temp,
        type_ann: None,
        init: materialized,
        is_var: false,
    };
    let original = std::mem::replace(s, Stmt::Multi(Vec::new()));
    *s = Stmt::Multi(vec![hoist, original]);
}

/// Walk the first-evaluation chain from `root`, looking for a call
/// whose trailing single spread has a source that is not an evident
/// array (a call, a member, or an Ident without array evidence) and
/// whose fixed prefix args are effect-free. Returns the Spread expr
/// id (to rewrite in place) and the source expr id (the temp's init).
fn find_hoist_target(
    ast: &Ast,
    root: ExprId,
    evident: &HashSet<String>,
) -> Option<(ExprId, ExprId)> {
    let mut cur = root;
    loop {
        let Expr::Call { callee, args } = ast.get_expr(cur) else {
            return None;
        };
        // The callee evaluates before the args — it must be
        // effect-free for the hoist to move ahead of it.
        match ast.get_expr(*callee) {
            Expr::Ident(_) => {}
            Expr::Member { obj, .. } if matches!(ast.get_expr(*obj), Expr::Ident(_)) => {}
            _ => return None,
        }
        let (&last, prefix) = args.split_last()?;
        if let Expr::Spread { expr } = ast.get_expr(last) {
            let spread_count = args
                .iter()
                .filter(|a| matches!(ast.get_expr(**a), Expr::Spread { .. }))
                .count();
            if spread_count == 1
                && !matches!(ast.get_expr(*expr), Expr::Ident(n) if evident.contains(n))
                && prefix.iter().all(|p| is_effect_free(ast, *p))
            {
                return Some((last, *expr));
            }
            // A spread call that doesn't qualify (multiple spreads,
            // effectful prefix) stays on the loud reject.
            return None;
        }
        // No spread here — descend into the first-evaluated arg.
        cur = *args.first()?;
    }
}

/// Evaluation with no observable side effect — safe to reorder the
/// hoisted source ahead of these.
fn is_effect_free(ast: &Ast, eid: ExprId) -> bool {
    matches!(
        ast.get_expr(eid),
        Expr::Ident(_) | Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Null
    )
}
