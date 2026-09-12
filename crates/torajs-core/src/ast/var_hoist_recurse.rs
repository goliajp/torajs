//! The nested-statement descent of the `var` hoist — displaced from
//! [`super::var_hoist`] (rotation 637, file-size: the host sat at 485
//! prod lines and the global-function knife needs room there).
//! Body verbatim; `collect_and_rewrite_var` and this walker are
//! mutually recursive across the two files.

use super::var_hoist::collect_and_rewrite_var;
use super::{Expr, ExprId, Stmt};

pub(super) fn hoist_recurse_stmt(
    s: &mut Stmt,
    hoisted: &mut std::collections::BTreeMap<String, Option<String>>,
    exprs: &mut Vec<Expr>,
    multi: &std::collections::HashSet<String>,
) {
    match s {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            hoist_recurse_stmt(then_branch, hoisted, exprs, multi);
            if let Some(eb) = else_branch.as_deref_mut() {
                hoist_recurse_stmt(eb, hoisted, exprs, multi);
            }
        }
        Stmt::Block(b) => {
            collect_and_rewrite_var(b, hoisted, exprs, true, multi);
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            hoist_recurse_stmt(body, hoisted, exprs, multi);
        }
        Stmt::Labeled { body, .. } => {
            hoist_recurse_stmt(body, hoisted, exprs, multi);
        }
        Stmt::For { body, init, .. } => {
            // Special-case: a single `var i = 0` in the for-init slot
            // hoists `i` to the enclosing fn-body. Replace the
            // for-init with the equivalent `i = 0` assignment so the
            // loop semantic stays unchanged.
            if let Some(init_box) = init.as_deref_mut() {
                if let Stmt::LetDecl {
                    name,
                    init: init_id,
                    is_var: true,
                    ..
                } = init_box
                {
                    // §14.7.4 — a for-head `var` shares the fn-level
                    // hoist domain, annotation or not. The old typed
                    // escape hatch mirrored the top-level one by
                    // flipping `is_var` off in place, but the mirror
                    // does not hold here: a top-level escape `let`
                    // stays visible to the rest of the fn body, while
                    // a for-head `let` is scoped to the loop — so
                    // `for (var x: any = 1; …)` left `x` unreadable
                    // after the loop (401-05). Same reasoning as the
                    // rotation-264 nested-list fix: the for head IS a
                    // nested position, and nested vars always hoist.
                    let nm = name.clone();
                    let init_eid = *init_id;
                    hoisted.insert(nm.clone(), None);
                    if !matches!(exprs[init_eid.0 as usize], Expr::Uninit) {
                        let target_id = ExprId(exprs.len() as u32);
                        exprs.push(Expr::Ident(nm));
                        let assign_id = ExprId(exprs.len() as u32);
                        exprs.push(Expr::Assign {
                            target: target_id,
                            value: init_eid,
                        });
                        *init_box = Stmt::Expr(assign_id);
                    } else {
                        *init = None;
                    }
                } else {
                    hoist_recurse_stmt(init_box, hoisted, exprs, multi);
                }
            }
            hoist_recurse_stmt(body, hoisted, exprs, multi);
        }
        Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => {
            hoist_recurse_stmt(body, hoisted, exprs, multi);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            collect_and_rewrite_var(body, hoisted, exprs, true, multi);
            collect_and_rewrite_var(catch_body, hoisted, exprs, true, multi);
            if let Some(fb) = finally_body {
                collect_and_rewrite_var(fb, hoisted, exprs, true, multi);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for case in cases {
                collect_and_rewrite_var(&mut case.body, hoisted, exprs, true, multi);
            }
            if let Some(dflt) = default {
                collect_and_rewrite_var(dflt, hoisted, exprs, true, multi);
            }
        }
        Stmt::Multi(inner) => {
            collect_and_rewrite_var(inner, hoisted, exprs, true, multi);
        }
        // A `var` reached here as a statement in its own right, not
        // as an element of a list. §14.6 / §14.7 / §14.13 take a
        // `Statement` and a VariableStatement is one, so
        // `if (c) var x = 1;` and its loop / labelled spellings are
        // ordinary JavaScript — but the rewrite that turns a `var`
        // into a prelude slot plus an assignment lives in
        // `collect_and_rewrite_var`, which walks LISTS. Every other
        // `var` in the language sits in one, which is why this arm
        // used to read "handled by the caller's match arm": true of
        // the list caller, and of no other. The ones arriving here
        // reached ssa_lower still `is_var: true` and died on its
        // catch-all ("statement shape not yet implemented"), taking
        // `if (true) var x = 1;` with them.
        //
        // A list of one is all it needs. `Multi` is the shape for a
        // compiler-made sequence that shares the parent's scope,
        // which is exactly right: the hoist has already moved the
        // binding, and what stays behind is an assignment that must
        // land where the declaration stood. An uninitialized `var x;`
        // rewrites to nothing and leaves an empty `Multi`, a no-op.
        Stmt::LetDecl { is_var: true, .. } => {
            let mut one = vec![std::mem::replace(s, Stmt::Multi(Vec::new()))];
            collect_and_rewrite_var(&mut one, hoisted, exprs, true, multi);
            *s = Stmt::Multi(one);
        }
        // `let` / `const` are block-scoped and hoist nothing.
        Stmt::LetDecl { .. } => {}
        // FnDecl is its own scope — don't descend (handled by the
        // top-level pass that hoists separately per fn body).
        Stmt::FnDecl { .. } => {}
        // Terminal stmts with no nested stmt list.
        _ => {}
    }
}
