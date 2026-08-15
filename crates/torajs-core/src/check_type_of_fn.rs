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
    for (p, ty) in params.iter().zip(param_tys.iter()) {
        if let Err(e) = checker.declare(
            p.name.clone(),
            LocalInfo {
                ty: ty.clone(),
                mutable: true,
                moved: false,
                borrowed: false,
                declared_class: None,
                builtin_mv: false,
            },
        ) {
            checker.errors.push_err(e);
        }
    }
    for s in &body {
        checker.check_stmt(ast, s);
    }
    checker.expected_return = saved_return;
    checker.scopes = saved_scopes;
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
    // RFC 20260714-objlit-accessor blade 1 — an object-literal method's
    // receiver is a param of the LIFTED fn, not of the method the user
    // wrote: `o.m()` passes no receiver. It still has to be declared
    // into the body's scope (that is what makes `this.a` resolve), so
    // only the VALUE type sheds it.
    let takes_recv = real_params.first().is_some_and(|p| p.name == "__this");
    let value_ty = if takes_recv {
        Type::Function(param_tys.iter().skip(1).cloned().collect(), ret_ty.clone())
    } else {
        fn_ty
    };
    // RFC 20260708-closure-argv-face — a full-arguments closure's
    // PUBLIC type is `(...args: any[]) => R`: any call arity is
    // legal and every call must ride the boxed dual entry (whose
    // adapter feeds real argc + argv into the synthetic params).
    // The rest-tail type reuses the whole variadic track — call
    // admit, assignability, and the SSA boxed-lane registration.
    let value_ty = if ast.closure_argv_fns.contains(fn_name) {
        Type::Function(vec![Type::Rest(Box::new(Type::Any))], ret_ty.clone())
    } else {
        value_ty
    };
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
        } else if cap.starts_with("__") || crate::check::is_known_builtin_global(cap) {
            // Same carve-outs as the undeclared-read lane: synthetic
            // names and name-keyed builtin globals stay hard errors.
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
    let sig = closure_sig(ast, &fn_name, &checker.aliases)?;
    let ClosureSig {
        real_params,
        param_tys,
        ret_ty,
        body,
        value_ty,
    } = sig;
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
        let _ = checker.declare(
            sn.clone(),
            LocalInfo {
                ty: value_ty.clone(),
                mutable: true,
                moved: false,
                borrowed: false,
                declared_class: None,
                builtin_mv: false,
            },
        );
        active = Some(sn.clone());
        checker.scopes.push(HashMap::new());
    }
    for (cap_name, cap_ty) in &cap_tys {
        let _ = checker.declare(
            cap_name.clone(),
            LocalInfo {
                ty: cap_ty.clone(),
                mutable: true,
                moved: false,
                borrowed: false,
                declared_class: None,
                builtin_mv: false,
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
        let _ = checker.declare(
            p.name.clone(),
            LocalInfo {
                ty: binding_ty,
                mutable: true,
                moved: false,
                borrowed: false,
                declared_class: None,
                builtin_mv: false,
            },
        );
    }
    if active.is_none() {
        if let Some(sn) = &saved_self_name {
            if cap_tys.iter().any(|(n, _)| n == sn) && !real_params.iter().any(|p| p.name == *sn) {
                active = Some(sn.clone());
            }
        }
    }
    checker.self_name_active = active;
    for s in &body {
        checker.check_stmt(ast, &s.clone());
    }
    checker.self_name_active = saved_self_name;
    checker.expected_return = saved_return;
    checker.scopes = saved_scopes;
    Ok(value_ty)
}
