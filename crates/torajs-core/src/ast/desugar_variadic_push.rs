//! Variadic `arr.push(...)` / `arr.unshift(...)` splitting pass.
//!
//! Chunk 352 — extracted from ast.rs. Rewrites multi-arg push/unshift
//! calls with an Ident receiver into N sequential single-arg calls so
//! the SSA lowering stays variadic-agnostic. The entry
//! `desugar_variadic_push` is `pub` because torajs-cli main /
//! cmd_build / lsp reach it at `torajs_core::ast::desugar_variadic_push`;
//! the `pub use` in ast.rs preserves the path. Everything else
//! (`split_variadic_push_call` + the two `rewrite_variadic_push_*`
//! walkers) is a private helper.

use super::{Expr, ExprId, Stmt};
use crate::ast::Ast;

/// V3-18 wedge — rewrite multi-arg `arr.push(a, b, c)` /
/// `arr.unshift(a, b, c)` into N consecutive single-arg calls
/// at the stmt level. Per JS spec the calls run in order on the
/// same array; this preserves that semantic without requiring
/// the SSA-level push lowering to be variadic-aware.
///
/// Subset limitation: only Ident receivers are rewritten. A
/// complex receiver like `o.field.push(a, b)` is left alone
/// (would re-evaluate `o.field` per call rather than once);
/// users with that shape can hoist into a temp.
pub fn desugar_variadic_push(ast: &mut Ast) {
    let exprs_snapshot = ast.exprs.clone();
    let mut stmts = std::mem::take(&mut ast.stmts);
    rewrite_variadic_push_in_stmts(&mut stmts, &exprs_snapshot, &mut ast.exprs);
    ast.stmts = stmts;
}

/// S157 helper — try to split a multi-arg ident-receiver push/unshift
/// `Expr::Call` (referenced by `eid`) into hoisted single-arg sibling
/// calls plus a single-arg replacement call returned as a fresh
/// `ExprId`. Returns `None` if `eid` isn't the recognized shape.
///
/// Used for both statement-position (S132 original — return value
/// discarded) and expression-position (S157 extension — return value
/// consumed by enclosing LetDecl / Return / etc., where push's spec
/// return = FINAL length matches the last hoisted call's return).
fn split_variadic_push_call(
    eid: ExprId,
    snapshot: &[Expr],
    out_exprs: &mut Vec<Expr>,
) -> Option<(Vec<Stmt>, ExprId)> {
    let e = &snapshot[eid.0 as usize];
    let Expr::Call { callee, args } = e else {
        return None;
    };
    if args.len() <= 1 {
        return None;
    }
    let Expr::Member { obj, name } = &snapshot[callee.0 as usize] else {
        return None;
    };
    if !matches!(name.as_str(), "push" | "unshift") {
        return None;
    }
    if !matches!(snapshot[obj.0 as usize], Expr::Ident(_)) {
        return None;
    }
    let callee_id = *callee;
    let mut args_clone = args.clone();
    // unshift(a, b, c) prepends a, b, c such that after the call a is
    // at index 0. Equivalent to sequential unshift(c), unshift(b),
    // unshift(a) — REVERSE order. push needs no reorder.
    if name == "unshift" {
        args_clone.reverse();
    }
    let last_arg = args_clone.pop().unwrap();
    let mut hoisted: Vec<Stmt> = Vec::with_capacity(args_clone.len());
    for a in args_clone {
        let new_call = Expr::Call {
            callee: callee_id,
            args: vec![a],
        };
        let new_eid = ExprId(out_exprs.len() as u32);
        out_exprs.push(new_call);
        hoisted.push(Stmt::Expr(new_eid));
    }
    let last_call = Expr::Call {
        callee: callee_id,
        args: vec![last_arg],
    };
    let last_eid = ExprId(out_exprs.len() as u32);
    out_exprs.push(last_call);
    Some((hoisted, last_eid))
}

fn rewrite_variadic_push_in_stmts(
    stmts: &mut Vec<Stmt>,
    snapshot: &[Expr],
    out_exprs: &mut Vec<Expr>,
) {
    let mut i = 0;
    while i < stmts.len() {
        let replacement: Option<Stmt> = match &stmts[i] {
            Stmt::Expr(eid) => {
                // Statement-position: return value discarded; hoisted
                // single-arg calls fully replace the multi-arg call.
                if let Some((mut hoisted, last_eid)) =
                    split_variadic_push_call(*eid, snapshot, out_exprs)
                {
                    hoisted.push(Stmt::Expr(last_eid));
                    Some(Stmt::Multi(hoisted))
                } else {
                    None
                }
            }
            // S157 — expression-position let / const: `const rv =
            // r.push(a, b, c)`. push's spec return is the FINAL length
            // after all elements appended, so the last hoisted call's
            // return value is what user expects. Hoist (n-1) single-arg
            // calls before, leave a single-arg push as the LetDecl's
            // init expression.
            Stmt::LetDecl {
                mutable,
                name,
                type_ann,
                init,
                is_var,
            } => {
                if let Some((mut hoisted, last_eid)) =
                    split_variadic_push_call(*init, snapshot, out_exprs)
                {
                    hoisted.push(Stmt::LetDecl {
                        mutable: *mutable,
                        name: name.clone(),
                        type_ann: type_ann.clone(),
                        init: last_eid,
                        is_var: *is_var,
                    });
                    Some(Stmt::Multi(hoisted))
                } else {
                    None
                }
            }
            // S157 — `return r.push(a, b, c)` mirrors the LetDecl case:
            // last hoisted call's value = final length per spec.
            Stmt::Return(Some(eid)) => {
                if let Some((mut hoisted, last_eid)) =
                    split_variadic_push_call(*eid, snapshot, out_exprs)
                {
                    hoisted.push(Stmt::Return(Some(last_eid)));
                    Some(Stmt::Multi(hoisted))
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(r) = replacement {
            stmts[i] = r;
            // Newly-emitted Multi only holds single-arg push calls
            // already in their final shape; skip recursion (and
            // its snapshot lookups, which would index beyond the
            // pre-rewrite snapshot length).
            i += 1;
            continue;
        }
        // Recurse into nested stmt lists.
        match &mut stmts[i] {
            Stmt::Block(inner) | Stmt::Multi(inner) => {
                rewrite_variadic_push_in_stmts(inner, snapshot, out_exprs);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                rewrite_variadic_push_in_stmt_box(then_branch, snapshot, out_exprs);
                if let Some(eb) = else_branch {
                    rewrite_variadic_push_in_stmt_box(eb, snapshot, out_exprs);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                rewrite_variadic_push_in_stmt_box(body, snapshot, out_exprs);
            }
            Stmt::For { init, body, .. } => {
                if let Some(b) = init {
                    rewrite_variadic_push_in_stmt_box(b, snapshot, out_exprs);
                }
                rewrite_variadic_push_in_stmt_box(body, snapshot, out_exprs);
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    rewrite_variadic_push_in_stmts(&mut c.body, snapshot, out_exprs);
                }
                if let Some(db) = default {
                    rewrite_variadic_push_in_stmts(db, snapshot, out_exprs);
                }
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                rewrite_variadic_push_in_stmts(body, snapshot, out_exprs);
                rewrite_variadic_push_in_stmts(catch_body, snapshot, out_exprs);
                if let Some(fb) = finally_body {
                    rewrite_variadic_push_in_stmts(fb, snapshot, out_exprs);
                }
            }
            Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => {
                rewrite_variadic_push_in_stmt_box(body, snapshot, out_exprs);
            }
            Stmt::FnDecl { body, .. } => {
                rewrite_variadic_push_in_stmts(body, snapshot, out_exprs);
            }
            Stmt::ClassDecl {
                ctor,
                methods,
                static_methods,
                ..
            } => {
                if let Some(c) = ctor {
                    rewrite_variadic_push_in_stmts(&mut c.body, snapshot, out_exprs);
                }
                for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                    rewrite_variadic_push_in_stmts(&mut m.body, snapshot, out_exprs);
                }
            }
            _ => {}
        }
        i += 1;
    }
}

fn rewrite_variadic_push_in_stmt_box(
    s: &mut Box<Stmt>,
    snapshot: &[Expr],
    out_exprs: &mut Vec<Expr>,
) {
    let mut owned = std::mem::replace(s.as_mut(), Stmt::Break);
    let mut wrapper = vec![owned];
    rewrite_variadic_push_in_stmts(&mut wrapper, snapshot, out_exprs);
    owned = wrapper.into_iter().next().unwrap();
    **s = owned;
}
