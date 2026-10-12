//! `Expr::ArrowFn { params, return_type, body }` + `Expr::Closure
//! { fn_name, captures }` typecheck pulled out of
//! [`crate::check::Checker::type_of_inner`]'s corresponding arms
//! as chunk-109 of the type_of_inner decomp.
//!
//! - **ArrowFn** (legacy / non-capturing path) — `lift_arrow_fns`
//!   normally rewrites all arrows into lifted `FnDecl`s; arrows
//!   that survive (non-capturing) take this body-walk path. Body
//!   sees its own params only. Replace scope stack with fresh +
//!   replace `expected_return`; declare each param; check_stmt
//!   each body stmt; restore.
//! - **Closure** (lifted) — `fn_name` references a lifted
//!   `Stmt::FnDecl`. Resolve capture types in the OUTER scope,
//!   record them in `closure_captures` for the lowerer, then walk
//!   the lifted FnDecl's body in a fresh scope with captures + real
//!   params (skipping the leading `__env` synthetic param) bound.

use std::collections::HashMap;

use crate::ast::{Ast, Param, Stmt};
use crate::check::{Checker, DiagPush, LocalInfo, Type, build_fn_type};

mod value_ty;

pub(crate) fn check_arrow_fn(
    checker: &mut Checker,
    ast: &Ast,
    params: &[Param],
    return_type: &Option<String>,
    body: &[Stmt],
) -> Result<Type, String> {
    let params = params.to_vec();
    let return_type = return_type.clone();
    let body = body.to_vec();
    let fn_ty = build_fn_type("<arrow>", &params, &return_type, &checker.aliases)?;
    let Type::Function(param_tys, ret_ty) = fn_ty.clone() else {
        unreachable!("build_fn_type returned non-Function");
    };
    let saved_scopes = std::mem::replace(&mut checker.scopes, vec![HashMap::new()]);
    let saved_return = checker.expected_return.replace(*ret_ty);
    checker.toplevel_captures.push(Default::default());
    for (p, ty) in params.iter().zip(param_tys.iter()) {
        if let Err(e) = checker.declare(p.name.clone(), LocalInfo::plain(ty.clone())) {
            checker.errors.push_err(e);
        }
    }
    for s in &body {
        checker.check_stmt(ast, s);
    }
    checker.expected_return = saved_return;
    checker.scopes = saved_scopes;
    checker.toplevel_captures.pop();
    Ok(fn_ty)
}

/// Everything about a lifted closure that its FnDecl's ANNOTATIONS fix,
/// as opposed to what walking its body would tell you: the user-facing
/// params, their types, the return type, the body, and the type the
/// closure VALUE has.
///
/// Two callers read it — [`check_closure`], and the let-decl
/// pre-declare a self-referential init needs (`const f = n => f(n - 1)`
/// has to resolve the capture `f` before anything can walk the body).
/// Both read this one function, so the value type they agree on has no
/// way to drift apart.
pub(crate) struct ClosureSig {
    pub(crate) real_params: Vec<Param>,
    pub(crate) param_tys: Vec<Type>,
    pub(crate) ret_ty: Type,
    pub(crate) body: Vec<Stmt>,
    pub(crate) value_ty: Type,
}

pub(crate) fn closure_sig(
    ast: &Ast,
    fn_name: &str,
    aliases: &HashMap<String, Type>,
) -> Result<ClosureSig, String> {
    let fn_decl = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
            ..
        } if name == fn_name => Some((params.clone(), return_type.clone(), body.clone())),
        _ => None,
    });
    let Some((params, return_type, body)) = fn_decl else {
        return Err(format!("closure target `{fn_name}` has no FnDecl"));
    };
    // Skip the leading `__env` param — captures replace it.
    let real_params: Vec<Param> = params.iter().skip(1).cloned().collect();
    let fn_ty = build_fn_type(fn_name, &real_params, &return_type, aliases)?;
    let Type::Function(param_tys, ret_ty) = fn_ty.clone() else {
        unreachable!("build_fn_type returned non-Function");
    };
    let value_ty = value_ty::closure_value_ty(ast, fn_name, &real_params, fn_ty);
    Ok(ClosureSig {
        real_params,
        param_tys,
        ret_ty: *ret_ty,
        body,
        value_ty,
    })
}

pub(crate) fn check_closure(
    checker: &mut Checker,
    ast: &Ast,
    eid: crate::ast::ExprId,
    fn_name: &str,
    captures: &[String],
) -> Result<Type, String> {
    let fn_name = fn_name.to_string();
    if let Some(t) = checker.closure_walk_memo.get(&(eid, fn_name.clone())) {
        return Ok(t.clone());
    }
    if checker.defer_closure_bodies > 0 {
        // Skeleton walk: the statement walks the body once its binding
        // is in scope (`check_stmt_let_decl::predeclare_self_ref_closure`).
        return closure_sig(ast, &fn_name, &checker.aliases).map(|s| s.value_ty);
    }
    let captures = captures.to_vec();
    let mut cap_tys: Vec<(String, Type)> = Vec::with_capacity(captures.len());
    for cap in &captures {
        if let Some(info) = checker.lookup(cap) {
            cap_tys.push((cap.clone(), info.ty));
        } else if checker.globals.contains_key(cap) {
            // K.3/K.4/K.6 promoted data global (or hoisted fn) — not a
            // capture: the body ident resolves through the same globals
            // fallback named-fn bodies use, so it needs no scope entry.
        } else if let Some(ty) = checker.hoisted_closure_lets.get(cap) {
            // A closure binding declared LATER in this same statement
            // list (`crate::check_hoist_closure_lets`) — a real capture,
            // typed from the FnDecl its own declaration will bind.
            cap_tys.push((cap.clone(), ty.clone()));
        } else if cap
            .strip_prefix("__class_")
            .or_else(|| cap.strip_prefix("__proto_"))
            .is_some_and(|c| checker.class_names.contains(c))
        {
            // r505 (A12) — a class object / prototype cell is not a
            // capture: the body's read resolves through the class
            // registry (`class_get` / `proto_get`), the same way a
            // named-fn body reads it, and the lowering's capture
            // filter (`class_sentinel_name`) skips the env slot.
        } else if checker.is_synthesized_dunder(cap) || crate::check::is_known_builtin_global(cap) {
            // Same carve-outs as the undeclared-read lane: names TR
            // MINTED (by provenance, not by the `__` prefix) and
            // name-keyed builtin globals stay hard errors.
            return Err(format!(
                "closure `{fn_name}` references unknown identifier `{cap}`"
            ));
        } else {
            // RFC 20260730-undeclared-ident 刀 3 — a capture that
            // resolves nowhere (§6.2.5.5). Not a compile reject: skip
            // the env slot and record it; the body's Ident read takes
            // the undeclared-read mark lane (runtime ReferenceError),
            // and check_monomorph prunes the name from the owned
            // AST's capture list so env materialization never sees it.
            checker
                .unresolved_captures
                .entry(eid)
                .or_default()
                .push(cap.clone());
        }
    }
    checker
        .closure_captures
        .insert(fn_name.clone(), cap_tys.clone());
    let ty = check_closure_body(checker, ast, eid, &fn_name, cap_tys)?;
    // the replace lane's view twin is a clone of this body with no
    // construction site of its own: checked here, in the same scope
    if let Some(twin) = ast.regex_replace_view_fns.get(&fn_name).cloned() {
        check_closure(checker, ast, eid, &twin, &captures)?;
    }
    Ok(ty)
}

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
    let cap_outer: HashMap<String, LocalInfo> = cap_tys
        .iter()
        .filter_map(|(n, _)| Some((n.clone(), checker.lookup(n)?)))
        .collect();
    let saved_scopes = std::mem::replace(&mut checker.scopes, vec![HashMap::new()]);
    let saved_return = checker.expected_return.replace(ret_ty);
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
    for (cap_name, cap_ty) in &cap_tys {
        let _ = checker.declare(
            cap_name.clone(),
            LocalInfo {
                fn_slot: cap_outer.get(cap_name).and_then(|i| i.fn_slot.clone()),
                union: cap_outer
                    .get(cap_name)
                    .map(|i| i.union.clone())
                    .unwrap_or_default(),
                held_any: cap_outer.get(cap_name).is_some_and(|i| i.held_any),
                ann: cap_outer.get(cap_name).and_then(|i| i.ann.clone()),
                decl: cap_outer.get(cap_name).and_then(|i| i.decl.clone()),
                ..LocalInfo::plain(cap_ty.clone())
            },
        );
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
    let saved_hoists = crate::check_hoist_closure_lets::enter(checker, ast, &body);
    for s in &body {
        checker.check_stmt(ast, &s.clone());
    }
    checker.hoisted_closure_lets = saved_hoists;
    checker.self_name_active = saved_self_name;
    checker.expected_return = saved_return;
    checker.scopes = saved_scopes;
    checker.toplevel_captures.pop();
    if checker.speculative_depth == 0 {
        checker
            .closure_walk_memo
            .insert((eid, fn_name), value_ty.clone());
    }
    Ok(value_ty)
}
