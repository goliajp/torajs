//! What a row does when the slot's tail begins in front of parameters
//! it declared.
//!
//! A rest parameter is the one thing a join cannot widen INTO: where a
//! row's fixed parameters end decides where its rest begins. When the
//! rows of one slot disagree about that — `f(x, y = 5)` beside
//! `f(x, ...r)` — the tail has to begin where the VARIADIC row says it
//! does, because that row has no other reading of the argument list:
//! `f(1, 2, 3)` gives it `r = [2, 3]` and nothing else will do.
//!
//! The other row's `y` is then inside the tail, and it reads it back
//! out at the head of its own body:
//!
//! ```text
//! function __cm_A__f(__this, x, ...__slotrest) {
//!   let y = __slotrest[0]
//!   if (y === undefined) y = 5
//!   return x + y
//! }
//! ```
//!
//! Both statements are ones the language already writes: an omitted
//! argument reads back as `undefined` (§10.2.11) because the tail
//! simply is not that long, and the guard is the same one
//! `materialize_expr_defaults` splices for a plain function's default.
//!
//! The `__dispatch_` stub of such a slot has to be rebuilt too. It
//! forwards to the base owner BY NAME, and a name the tail swallowed
//! no longer exists — the stub's body grew a `throw ReferenceError`
//! where the forward used to be, which is what withdrew this whole
//! rewrite once before.

use super::super::super::{Ast, Expr, Param, Stmt};
use super::SlotShape;

/// One declaration's parameters rewritten to the joined shape, and
/// the names the tail begins in front of.
pub(super) struct RowShape {
    pub params: Vec<Param>,
    /// Parameters the slot's tail swallowed, in declaration order —
    /// read back out of it at the head of the body.
    pub rebound: Vec<String>,
}

/// One declaration's parameters rewritten to the joined shape, or
/// `None` when it already had it. Existing parameters keep their
/// names — the body reads them by name — and only the annotation
/// moves; the positions the row never declared arrive as pads, and so
/// does a rest tail the row never declared.
pub(super) fn rebuild(have: &[Param], shape: &SlotShape) -> Option<RowShape> {
    let (fixed, had_rest) = super::split_rest(have);
    let mut out: Vec<Param> = Vec::with_capacity(shape.fixed.len() + 1);
    let mut changed = fixed.len() != shape.fixed.len();
    for (i, want) in shape.fixed.iter().enumerate() {
        match fixed.get(i) {
            Some(p) => {
                changed |= p.type_ann != *want;
                out.push(Param {
                    type_ann: want.clone(),
                    ..p.clone()
                });
            }
            None => out.push(Param {
                name: format!("__slotpad{i}"),
                type_ann: Some("any".to_string()),
                default: None,
                is_rest: false,
            }),
        }
    }
    let rebound: Vec<String> = fixed[shape.fixed.len().min(fixed.len())..]
        .iter()
        .map(|p| p.name.clone())
        .collect();
    match (&shape.rest, had_rest) {
        (Some(ann), Some(p)) => {
            changed |= p.type_ann.as_ref() != Some(ann);
            out.push(Param {
                type_ann: Some(ann.clone()),
                ..p.clone()
            });
        }
        (Some(ann), None) => {
            changed = true;
            out.push(Param {
                name: TAIL.to_string(),
                type_ann: Some(ann.clone()),
                default: None,
                is_rest: true,
            });
        }
        (None, Some(_)) => unreachable!("a row with a rest joins to a shape with one"),
        (None, None) => {}
    }
    changed.then_some(RowShape {
        params: out,
        rebound,
    })
}

/// The name a row that never declared a rest carries the slot's tail
/// under. A row that DID declare one keeps its own name, and such a
/// row never rebinds — its fixed parameters are where the tail begins.
pub(super) const TAIL: &str = "__slotrest";

/// `let <name> = __slotrest[j]`, one per swallowed parameter, in
/// declaration order — so a later default that reads an earlier
/// parameter sees the value that parameter's own guard settled
/// (§9.2 ordering), the same reason the guards go in that order.
pub(super) fn rebind_stmts(ast: &mut Ast, names: &[String]) -> Vec<Stmt> {
    names
        .iter()
        .enumerate()
        .map(|(j, name)| {
            let obj = ast.add_expr(Expr::Ident(TAIL.to_string()));
            let index = ast.add_expr(Expr::Number(j as f64));
            let init = ast.add_expr(Expr::Index { obj, index });
            Stmt::LetDecl {
                mutable: true,
                name: name.clone(),
                type_ann: Some("any".to_string()),
                init,
                is_var: false,
            }
        })
        .collect()
}

/// The `__dispatch_` stub bodies that have to be rebuilt, keyed by
/// name.
///
/// A stub forwards to the base owner BY NAME, and a name the tail
/// swallowed no longer exists — the stub's body grew a
/// `throw ReferenceError` where the forward used to be, which is what
/// withdrew this rewrite once before. It never runs (SSA intercepts
/// the call and dispatches on the receiver's vtable slot), but it has
/// to typecheck, and it has to say something true.
///
/// The callee stays the one already written there — the base owner's
/// `__cm_`, whose parameter list is this same joined one — and the
/// tail forwards as a spread, which `apply_rest_args` turns into
/// `Array.from(tail)` one pass later: the fresh array §10.4.2 asks
/// for, rather than the source handed straight through.
///
/// Read first, minted second: the walk that installs these borrows
/// the statement arena the minting needs.
pub(super) fn forward_bodies(
    ast: &mut Ast,
    plan: &std::collections::HashMap<String, super::RowPlan>,
) -> std::collections::HashMap<String, Vec<Stmt>> {
    let mut targets: Vec<(String, crate::ast::ExprId, Vec<Param>)> = Vec::new();
    for stmt in super::super::super::toplevel_stmts_flat(ast) {
        let Stmt::FnDecl { name, body, .. } = stmt else {
            continue;
        };
        let Some(row) = plan.get(name.as_str()) else {
            continue;
        };
        if row.rebound.is_empty() || !name.starts_with("__dispatch_") {
            continue;
        }
        let Some(Stmt::Return(Some(eid))) = body.first() else {
            continue;
        };
        let Expr::Call { callee, .. } = ast.get_expr(*eid) else {
            continue;
        };
        targets.push((name.clone(), *callee, row.params.clone()));
    }
    let mut out = std::collections::HashMap::new();
    for (name, callee, params) in targets {
        let mut args = vec![ast.add_expr(Expr::Ident("__this".to_string()))];
        for p in &params {
            let id = ast.add_expr(Expr::Ident(p.name.clone()));
            args.push(if p.is_rest {
                ast.add_expr(Expr::Spread { expr: id })
            } else {
                id
            });
        }
        let call = ast.add_expr(Expr::Call { callee, args });
        out.insert(name, vec![Stmt::Return(Some(call))]);
    }
    out
}
