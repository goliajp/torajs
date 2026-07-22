//! Non-Ident spread-source hoist (chunk 689, spread longtail #5).
//!
//! Every spread walk (fixed-arity direct expansion, Math.min/max,
//! xs.push) needs an Ident source — the expansion re-reads it per
//! slot. A call / member source (`sum2(...mk())`, `f(...o.arr)`)
//! hoists into a compiler temp ahead of the statement:
//!
//! ```text
//! console.log(sum2(...mk()));
//!   ⇒ Multi[ let __sps_N = mk();
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

use super::{Ast, Expr, ExprId, Stmt};

pub(super) fn apply_spread_hoist(ast: &mut Ast) {
    let mut counter = 0usize;
    let mut stmts = std::mem::take(&mut ast.stmts);
    for s in &mut stmts {
        rewrite_stmt(ast, s, &mut counter);
    }
    ast.stmts = stmts;
}

fn rewrite_stmt(ast: &mut Ast, s: &mut Stmt, counter: &mut usize) {
    // Recurse into nested bodies first, then try the statement's own
    // root expr (Expr / LetDecl-init / Return positions).
    match s {
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for c in stmts {
                rewrite_stmt(ast, c, counter);
            }
            return;
        }
        Stmt::FnDecl { body, .. } => {
            for c in body {
                rewrite_stmt(ast, c, counter);
            }
            return;
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_stmt(ast, then_branch, counter);
            if let Some(eb) = else_branch {
                rewrite_stmt(ast, eb, counter);
            }
            return;
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            rewrite_stmt(ast, body, counter);
            return;
        }
        Stmt::Labeled { body, .. } => {
            rewrite_stmt(ast, body, counter);
            return;
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                rewrite_stmt(ast, i, counter);
            }
            rewrite_stmt(ast, body, counter);
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
    let Some((spread_eid, src_eid)) = find_hoist_target(ast, root) else {
        return;
    };
    let temp = format!("__sps_{}", *counter);
    *counter += 1;
    let temp_ref = ast.add_expr(Expr::Ident(temp.clone()));
    ast.exprs[spread_eid.0 as usize] = Expr::Spread { expr: temp_ref };
    let hoist = Stmt::LetDecl {
        mutable: false,
        name: temp,
        type_ann: None,
        init: src_eid,
        is_var: false,
    };
    let original = std::mem::replace(s, Stmt::Multi(Vec::new()));
    *s = Stmt::Multi(vec![hoist, original]);
}

/// Walk the first-evaluation chain from `root`, looking for a call
/// whose trailing single spread has a non-Ident source and whose
/// fixed prefix args are effect-free. Returns the Spread expr id
/// (to rewrite in place) and the source expr id (the temp's init).
fn find_hoist_target(ast: &Ast, root: ExprId) -> Option<(ExprId, ExprId)> {
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
                && !matches!(ast.get_expr(*expr), Expr::Ident(_))
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
