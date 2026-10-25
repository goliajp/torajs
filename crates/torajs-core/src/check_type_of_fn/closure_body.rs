//! The body half of a lifted closure's check: the body walked in a
//! fresh scope with the captures and the real params bound.

use std::collections::HashMap;

use super::{ClosureSig, closure_sig};
use crate::ast::Ast;
use crate::check::{Checker, LocalInfo, Type};

/// The body half of [`check_closure`]: check lifted body `fn_name` with
/// the captures already typed. A clone minted after the main check
/// (RFC 20260917-plain-fn-new-target knife 2's construct face) has no
/// construction site whose scope is still live, so it passes the
/// original closure's recorded capture types here instead.
pub(crate) fn check_closure_body(
    checker: &mut Checker,
    ast: &Ast,
    eid: crate::ast::ExprId,
    fn_name: &str,
    cap_tys: Vec<(String, Type)>,
) -> Result<Type, String> {
    let fn_name = fn_name.to_string();
    let sig = closure_sig(ast, &fn_name, &checker.aliases)?;
    let ClosureSig {
        real_params,
        param_tys,
        ret_ty,
        body,
        value_ty,
    } = sig;
    // 551-03 — which of these captures ARE the top-level binding of
    // that name: resolved in main's own frame with no inner shadow,
    // or carried in from an enclosing closure that resolved it so.
    // The any-widen alias may retarget a call through such a capture
    // to the top-level const's lifted body, and through nothing else.
    let in_fn = checker.expected_return.is_some();
    let top_caps: std::collections::HashSet<String> = cap_tys
        .iter()
        .filter(|(n, _)| {
            if in_fn {
                checker
                    .toplevel_captures
                    .last()
                    .is_some_and(|s| s.contains(n))
            } else {
                checker.scopes.first().is_some_and(|s| s.contains_key(n))
                    && checker.scopes.iter().filter(|s| s.contains_key(n)).count() == 1
            }
        })
        .map(|(n, _)| n.clone())
        .collect();
    checker.toplevel_captures.push(top_caps);
    // a captured census slot stays that slot inside the closure, and a
    // captured union keeps its declared members and loose mark
    let (cap_frame, inherited) = checker.capture_frame(ast, eid, &cap_tys);
    let saved_scopes = std::mem::replace(&mut checker.scopes, vec![HashMap::new()]);
    let saved_narrows = checker.enter_closure_narrows();
    let saved_return = checker.expected_return.replace(ret_ty);
    let saved_owner = checker.ret_owner.replace(fn_name.clone());
    // §15.5.5 (RFC 20260810) — the fn-expression's self-name binds
    // the closure itself inside the body, in a scope of its OWN
    // between the enclosing environment and the body (the spec's
    // funcEnv): params shadow it via ordinary inner-scope lookup,
    // and a body-level `var n` / `let n` re-declaration is a legal
    // shadow, not a redeclare reject (scope-lex-open /
    // scope-var-open). `mutable: true` keeps the assign lane from a
    // compile reject — writes resolving to this binding are marked
    // via `self_name_active` and raise a runtime TypeError instead
    // (strict semantics; module code always is). An inner closure
    // reaches the name as an ordinary capture, but a write through
    // it still hits the same immutable binding — the active channel
    // rides the capture chain.
    let saved_self_name = checker.self_name_active.take();
    let mut active: Option<String> = None;
    if let Some(sn) = ast.closure_self_names.get(&fn_name) {
        let _ = checker.declare(sn.clone(), LocalInfo::plain(value_ty.clone()));
        active = Some(sn.clone());
        checker.scopes.push(HashMap::new());
    }
    let cap_scope = checker.scopes.len() - 1;
    for (cap_name, info) in cap_frame {
        let _ = checker.declare(cap_name, info);
    }
    for (p, ty) in real_params.iter().zip(param_tys.iter()) {
        // 刀 1 (RFC 20260815-fn-value-rest-spread) — the signature
        // spells a rest param as the `Rest(elem)` sentinel for call
        // sites; the body's binding is the packed array (§10.2.1.3),
        // mirroring `check_stmt_fn_decl`.
        let binding_ty = match ty {
            Type::Rest(elem) => Type::Array(elem.clone()),
            other => other.clone(),
        };
        let ann = p.type_ann.clone();
        let _ = checker.declare(
            p.name.clone(),
            LocalInfo {
                ann,
                decl: crate::check::FnSlot::param(&fn_name, &binding_ty),
                ..LocalInfo::plain(binding_ty)
            },
        );
    }
    if active.is_none()
        && let Some(sn) = &saved_self_name
        && cap_tys.iter().any(|(n, _)| n == sn)
        && !real_params.iter().any(|p| p.name == *sn)
    {
        active = Some(sn.clone());
    }
    checker.self_name_active = active;
    // a closure body is a function body like any other: its bindings
    // are all created on entry, so a closure binding may capture a
    // peer declared later in the same body
    let saved_hoists = crate::check_hoist_closure_lets::enter_body(checker, ast, &body);
    for s in &body {
        checker.check_stmt(ast, &s.clone());
    }
    checker.hoisted_closure_lets = saved_hoists;
    checker.self_name_active = saved_self_name;
    checker.expected_return = saved_return;
    checker.ret_owner = saved_owner;
    let cap_out = checker.capture_frame_out(cap_scope, &inherited);
    checker.scopes = saved_scopes;
    checker.leave_closure_narrows(ast, &fn_name, saved_narrows);
    checker.apply_capture_out(cap_out);
    checker.toplevel_captures.pop();
    if checker.speculative_depth == 0 {
        checker
            .closure_walk_memo
            .insert((eid, fn_name), value_ty.clone());
    }
    Ok(value_ty)
}
