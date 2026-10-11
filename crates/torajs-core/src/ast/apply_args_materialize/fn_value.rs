//! The fn-VALUE lane (module doc "Guarded faces", the ArrowFn bullet):
//! a closure or function expression has no static callee, so no call
//! site can pad its defaults, and every default the guard may run
//! moves into its body.
//!
//! Two entries. The pass itself plans every arrow still in the arena.
//! [`materialize_declared_fn_value`] serves a nested `function`
//! declaration that the capture router turns into a function
//! expression AFTER this pass ran: its declaration was planned on the
//! FnDecl lane (Phase C), which leaves a typed default on the call-site
//! pad — and once the declaration is a value read through a binding,
//! nothing pads it (`function outer(k) { function f(y: number = k)
//! {…} return f; }` bound `y` to undefined, or refused the
//! zero-argument call outright).

use super::*;

/// One fn value's conversion plan, in param order. `declared`: the
/// value was written as a `function` declaration, whose lane admits
/// a default that writes a binding (`n += 1`) — see
/// [`body_safe_default`].
pub(super) fn fn_value_conv(
    ast: &Ast,
    ei: usize,
    global_fns: &[String],
    declared: bool,
) -> Vec<(usize, String, Conv)> {
    let Expr::ArrowFn { params, .. } = &ast.exprs[ei] else {
        return Vec::new();
    };
    let fn_expr = ast.fn_expr_exprs.contains(&ExprId(ei as u32));
    let mut conv = Vec::new();
    // once one default runs in the body, every later one has to as
    // well: §10.2.11 evaluates them in parameter order, and a call-site
    // pad would run before the guards
    let mut moved = false;
    for (pi, p) in params.iter().enumerate() {
        let prior: Vec<String> = params[..pi].iter().map(|q| q.name.clone()).collect();
        // a fn-expr default reading the fn's own `arguments` is
        // meaningless at the call site, so it takes the body lane
        // even when it assigns (the FnDecl gate)
        let own_args = fn_expr && p.default.is_some_and(|d| reads_arguments(ast, d));
        let body_lane = declared || own_args || moved;
        if let Some(d) = converting_default(ast, p, global_fns, &prior, body_lane) {
            conv.push((pi, p.name.clone(), Conv::Any(d)));
            moved = true;
        } else if let Some((d, ann)) =
            converting_typed_default(ast, p, global_fns, &prior, body_lane)
        {
            conv.push((pi, p.name.clone(), Conv::TypedNarrow(d, ann)));
            moved = true;
        }
    }
    conv
}

/// Apply one fn value's plan: patch its params and splice the guards
/// at the head of its body.
pub(super) fn apply_fn_value_conv(ast: &mut Ast, ei: usize, conv: Vec<(usize, String, Conv)>) {
    let own_args =
        ast.fn_expr_exprs.contains(&ExprId(ei as u32)) && conv_reads_arguments(ast, &conv);
    let (guards, pads) = build_guards_and_pads(ast, conv);
    let Expr::ArrowFn { params, body, .. } = &mut ast.exprs[ei] else {
        unreachable!()
    };
    patch_params(params, pads);
    let mut b = std::mem::take(body);
    if own_args {
        body_arguments::split_body_arguments(ast, &mut b);
    }
    splice_guards(&mut b, guards, 0);
    if let Expr::ArrowFn { body, .. } = &mut ast.exprs[ei] {
        *body = b;
    }
}

/// See the module doc. The declaration's own Phase C guards already
/// sit at the head of the body, in param order; each guard this adds
/// goes in front of the first of them that belongs to a LATER
/// parameter, so the defaults still run in parameter order
/// (§10.2.11).
pub(crate) fn materialize_declared_fn_value(ast: &mut Ast, eid: ExprId) {
    let global_fns = top_fn_names(ast);
    let ei = eid.0 as usize;
    let conv = fn_value_conv(ast, ei, &global_fns, true);
    if conv.is_empty() {
        return;
    }
    let own_args = ast.fn_expr_exprs.contains(&eid) && conv_reads_arguments(ast, &conv);
    let names: Vec<String> = match &ast.exprs[ei] {
        Expr::ArrowFn { params, .. } => params.iter().map(|p| p.name.clone()).collect(),
        _ => unreachable!(),
    };
    let mut groups: Vec<(usize, Vec<Stmt>)> = Vec::new();
    let mut all_pads: ParamPads = Vec::new();
    for c in conv {
        let pi = c.0;
        let (guards, pads) = build_guards_and_pads(ast, vec![c]);
        groups.push((pi, guards));
        all_pads.extend(pads);
    }
    let Expr::ArrowFn { params, body, .. } = &mut ast.exprs[ei] else {
        unreachable!()
    };
    patch_params(params, all_pads);
    let mut b = std::mem::take(body);
    if own_args {
        body_arguments::split_body_arguments(ast, &mut b);
    }
    let list = guard_list_mut(&mut b);
    for (pi, group) in groups {
        let at = head_guard_position(ast, list, &names, pi);
        list.splice(at..at, group);
    }
    if let Expr::ArrowFn { body, .. } = &mut ast.exprs[ei] {
        *body = b;
    }
}

/// Where in `list`'s leading run of default guards the guard of
/// param `pi` belongs: before the first guard of a later param, or
/// at the end of the run.
fn head_guard_position(ast: &Ast, list: &[Stmt], names: &[String], pi: usize) -> usize {
    for (i, s) in list.iter().enumerate() {
        match s {
            Stmt::If {
                cond,
                else_branch: None,
                ..
            } => match guarded_param(ast, *cond, names) {
                Some(owner) if owner <= pi => {}
                _ => return i,
            },
            // the typed narrow that follows a carrier's guard
            Stmt::LetDecl { .. } if i > 0 && matches!(list[i - 1], Stmt::If { .. }) => {}
            _ => return i,
        }
    }
    list.len()
}

/// The param an `if (<p> === undefined) …` guard fills, as
/// [`build_default_guard`] spells its test.
fn guarded_param(ast: &Ast, cond: ExprId, names: &[String]) -> Option<usize> {
    let Expr::BinOp {
        op: BinOp::Eq,
        left,
        right,
    } = ast.get_expr(cond)
    else {
        return None;
    };
    if !matches!(ast.get_expr(*right), Expr::Ident(u) if u == "undefined") {
        return None;
    }
    let Expr::Ident(n) = ast.get_expr(*left) else {
        return None;
    };
    names.iter().position(|x| x == n)
}
