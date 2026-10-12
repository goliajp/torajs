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
    let Expr::ArrowFn { params, body, .. } = &ast.exprs[ei] else {
        return Vec::new();
    };
    let scope = match declared {
        true => captured_scope(ast, params, body, global_fns),
        false => Vec::new(),
    };
    let fn_expr = ast.fn_expr_exprs.contains(&ExprId(ei as u32));
    let mut conv = Vec::new();
    // once one default runs in the body, every later one has to as
    // well: §10.2.11 evaluates them in parameter order, and a call-site
    // pad would run before the guards
    let mut moved = false;
    for (pi, p) in params.iter().enumerate() {
        let mut prior: Vec<String> = params[..pi].iter().map(|q| q.name.clone()).collect();
        prior.extend(scope.iter().cloned());
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

/// What a nested function declaration reads from the scopes around it,
/// defaults included — the captures it takes once it becomes a closure.
/// An arrow default capturing one of them can run in its body: the
/// closure holding the arrow holds the binding too. Phase C and the
/// declared lane both admit them, so the two agree on what moves
/// ([`materialize_declared_fn_value`] depends on it).
pub(super) fn captured_scope(
    ast: &Ast,
    params: &[super::super::Param],
    body: &[Stmt],
    global_fns: &[String],
) -> Vec<String> {
    let mut bound: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
    bound.extend(global_fns.iter().cloned());
    crate::ast::free_vars::free_vars_of_fn(ast, &bound, &[], params, body)
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
    // a class constructor throws on [[Call]] before any default runs
    // (§10.2.1 step 2 precedes FunctionDeclarationInstantiation)
    let at = usize::from(
        b.first()
            .is_some_and(|s| crate::ast::capturing_classes::new_target::is_call_guard(ast, s)),
    );
    splice_guards(&mut b, guards, at);
    if let Expr::ArrowFn { body, .. } = &mut ast.exprs[ei] {
        *body = b;
    }
}

/// See the module doc. The declaration's own Phase C guards already
/// sit at the head of the body, in param order, and every one of them
/// belongs to a LATER parameter than any default converted here:
/// Phase C, once it moves one default into the body, moves every later
/// one it can (`collect_fn_conv`'s `moved`), and what it could not move
/// this lane cannot either. So this block goes in front of them all
/// and the defaults still run in parameter order (§10.2.11).
///
/// A default neither lane can move is refused: left on the pad, no call
/// of the value would evaluate it, and §10.2.11 owes the evaluation.
pub(crate) fn materialize_declared_fn_value(ast: &mut Ast, eid: ExprId, name: &str) {
    let global_fns = top_fn_names(ast);
    let ei = eid.0 as usize;
    let conv = fn_value_conv(ast, ei, &global_fns, true);
    if !conv.is_empty() {
        apply_fn_value_conv(ast, ei, conv);
    }
    let Expr::ArrowFn { params, .. } = &ast.exprs[ei] else {
        unreachable!()
    };
    let stranded = params
        .iter()
        .find(|p| p.default.is_some_and(|d| !is_absent_default(ast, d)));
    if let Some(p) = stranded {
        panic!(
            "the default of parameter `{}` of `{name}`, a nested \
             function that reads its enclosing scope (§10.2.11 evaluates it in the \
             function's own scope, and this shape cannot move there yet)",
            p.name
        );
    }
}
