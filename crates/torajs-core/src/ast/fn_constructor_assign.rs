//! `var F; F = function () {…}; new F();` — the assign-bound
//! spelling of a constructed function expression (the S13.2.2 test262
//! family writes exactly this: a bare multi-name `var` line, then one
//! top-level assignment that binds the fn-expr, then `new`).
//!
//! [`super::fn_constructor`] already covers the decl-init spelling
//! (`var F = function () {…}`) — the factory mint, the §10.2.2 step-8
//! return override and the `__this` receiver promotion all exist.
//! This sibling only teaches the binding RESOLUTION the second
//! spelling: a decl whose init is `Expr::Uninit` paired with the ONE
//! top-level `F = <fn-expr>` assignment. Everything past resolution
//! reuses the fn_constructor machinery unchanged.
//!
//! Gates (every refusal keeps today's loud reject):
//! - the name is declared once at top level, init `Uninit`, and the
//!   whole arena carries exactly ONE assignment to it — a rebinding
//!   (`F = g` later, or an assignment inside a function body) cannot
//!   be resolved statically, so those refuse;
//! - the assignment statement precedes, in top-level order, every
//!   statement whose free variables mention `__new_<name>` — a
//!   construct that runs before the binding exists must keep failing
//!   (the minted factory would otherwise call an uninitialized slot
//!   silently); a self-referencing ctor body trips this same gate
//!   (its own statement mentions the factory) and stays loud;
//! - for the `this`-writing promotion, every OTHER `Ident(<name>)`
//!   node must be a `.prototype` member read — the same use profile
//!   `promote_fn_expr_ctor_this` enforces — with the assignment
//!   target itself exempt (it IS the binding action, not a value
//!   escape).

use super::fn_constructor::{Constructible, returns_a_value};
use super::free_vars::free_vars_of_body;
use super::{Ast, Expr, ExprId, Param, Stmt};
use std::collections::HashSet;

/// An assign-bound fn-expr binding that passed the resolution gates.
struct AssignBound {
    name: String,
    /// The assigned `Expr::ArrowFn` node.
    fn_eid: ExprId,
    /// The assignment's target `Expr::Ident` node — exempt from the
    /// promotion use profile.
    target_eid: ExprId,
}

/// Resolve every gated assign-bound binding in the program.
fn assign_bound_bindings(ast: &Ast) -> Vec<AssignBound> {
    let mut out: Vec<AssignBound> = Vec::new();
    // The family's own spelling is a multi-name line (`var A, B;`),
    // which the parser emits as one `Stmt::Multi` — flatten a level so
    // those declarators are seen too.
    let decl_stmts = ast.stmts.iter().flat_map(|s| match s {
        Stmt::Multi(inner) => inner.iter(),
        other => std::slice::from_ref(other).iter(),
    });
    for st in decl_stmts {
        let Stmt::LetDecl {
            name,
            init,
            mutable: true,
            ..
        } = st
        else {
            continue;
        };
        if !matches!(ast.get_expr(*init), Expr::Uninit) {
            continue;
        }
        // Exactly one assignment to the name anywhere in the arena.
        // Dead nodes from earlier rewrites can only over-count, which
        // refuses — the safe direction.
        let mut assigned: Option<(ExprId, ExprId)> = None; // (target, value)
        let mut assign_count = 0usize;
        for e in &ast.exprs {
            if let Expr::Assign { target, value } = e
                && matches!(ast.get_expr(*target), Expr::Ident(n) if n == name)
            {
                assign_count += 1;
                assigned = Some((*target, *value));
            }
        }
        let Some((target_eid, value_eid)) = assigned else {
            continue;
        };
        if assign_count != 1 {
            continue;
        }
        if !ast.fn_expr_exprs.contains(&value_eid)
            || !matches!(ast.get_expr(value_eid), Expr::ArrowFn { .. })
        {
            continue;
        }
        // The assignment must be a top-level statement of its own —
        // an assignment buried in a function body runs at that
        // function's call time, which top-level order cannot speak
        // for.
        let Some(assign_idx) = ast.stmts.iter().position(|s| {
            matches!(s, Stmt::Expr(e)
                if matches!(ast.get_expr(*e), Expr::Assign { target, .. } if *target == target_eid))
        }) else {
            continue;
        };
        // Top-level order: the binding exists before anything that
        // can construct through it.
        let factory = format!("__new_{name}");
        let first_construct = ast.stmts.iter().position(|s| {
            free_vars_of_body(ast, &[], std::slice::from_ref(s))
                .iter()
                .any(|v| *v == factory)
        });
        if let Some(c_idx) = first_construct
            && c_idx <= assign_idx
        {
            continue;
        }
        out.push(AssignBound {
            name: name.clone(),
            fn_eid: value_eid,
            target_eid,
        });
    }
    out
}

/// Mirror of `promote_fn_expr_ctor_this` for the assign-bound
/// spelling: a constructed body that says `this` gets `__this: any`
/// as its first declared param, under the same
/// only-`.prototype`-reads use profile (the assignment target
/// exempt).
pub(super) fn promote_assign_bound_ctor_this(ast: &mut Ast) {
    let mut wanted: Vec<(String, ExprId, ExprId)> = Vec::new();
    for b in assign_bound_bindings(ast) {
        let Expr::ArrowFn { params, body, .. } = ast.get_expr(b.fn_eid) else {
            continue;
        };
        if params.iter().any(|p| p.is_rest || p.name == "__this") {
            continue;
        }
        let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
        if !free_vars_of_body(ast, &param_names, body)
            .iter()
            .any(|v| v == "__this")
        {
            continue;
        }
        let factory = format!("__new_{}", b.name);
        let constructed = ast.exprs.iter().any(
            |e| matches!(e, Expr::Call { callee, .. } if matches!(ast.get_expr(*callee), Expr::Ident(n) if *n == factory)),
        );
        if constructed {
            wanted.push((b.name, b.fn_eid, b.target_eid));
        }
    }

    for (name, fn_eid, target_eid) in wanted {
        let ident_eids: HashSet<u32> = ast
            .exprs
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e, Expr::Ident(n) if *n == name))
            .map(|(i, _)| i as u32)
            .collect();
        let mut good: HashSet<u32> = HashSet::new();
        good.insert(target_eid.0);
        for e in &ast.exprs {
            if let Expr::Member { obj, name: m } = e
                && ident_eids.contains(&obj.0)
                && m == "prototype"
            {
                good.insert(obj.0);
            }
        }
        if ident_eids.iter().any(|e| !good.contains(e)) {
            continue;
        }

        let Expr::ArrowFn { params, .. } = &mut ast.exprs[fn_eid.0 as usize] else {
            continue;
        };
        params.insert(
            0,
            Param {
                name: "__this".into(),
                type_ann: Some("any".into()),
                default: None,
                is_rest: false,
            },
        );
    }
}

/// Mirror of `collect_fn_expr_bindings` for the assign-bound
/// spelling: hand each gated binding to the factory mint as a
/// [`Constructible`].
pub(super) fn collect_assign_bound(ast: &Ast, candidates: &mut Vec<Constructible>) {
    for b in assign_bound_bindings(ast) {
        let Expr::ArrowFn { params, body, .. } = ast.get_expr(b.fn_eid) else {
            continue;
        };
        if candidates.iter().any(|c| c.name == b.name) {
            continue;
        }
        if params.iter().any(|p| p.is_rest) {
            continue;
        }
        let takes_this = params.first().is_some_and(|p| p.name == "__this");
        let user_params = if takes_this {
            &params[1..]
        } else {
            &params[..]
        };
        let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
        // A `this`-mentioning body that the promotion refused keeps
        // the loud reject.
        if !takes_this
            && free_vars_of_body(ast, &param_names, body)
                .iter()
                .any(|v| v == "__this")
        {
            continue;
        }
        candidates.push(Constructible {
            name: b.name.clone(),
            params: user_params
                .iter()
                .map(|p| Param {
                    type_ann: p.type_ann.clone().or_else(|| Some("any".into())),
                    ..p.clone()
                })
                .collect(),
            takes_this,
            returns_value: returns_a_value(body),
        });
    }
}
