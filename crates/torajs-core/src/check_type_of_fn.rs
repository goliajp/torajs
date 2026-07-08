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

pub(crate) fn check_closure(
    checker: &mut Checker,
    ast: &Ast,
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
        } else {
            return Err(format!(
                "closure `{fn_name}` references unknown identifier `{cap}`"
            ));
        }
    }
    checker
        .closure_captures
        .insert(fn_name.clone(), cap_tys.clone());
    let fn_decl = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
            ..
        } if name == &fn_name => Some((params.clone(), return_type.clone(), body.clone())),
        _ => None,
    });
    let Some((params, return_type, body)) = fn_decl else {
        return Err(format!("closure target `{fn_name}` has no FnDecl"));
    };
    // Skip the leading `__env` param — captures replace it.
    let real_params: Vec<Param> = params.iter().skip(1).cloned().collect();
    let user_fn_ty = build_fn_type(&fn_name, &real_params, &return_type, &checker.aliases)?;
    let Type::Function(param_tys, ret_ty) = user_fn_ty.clone() else {
        unreachable!();
    };
    let saved_scopes = std::mem::replace(&mut checker.scopes, vec![HashMap::new()]);
    let saved_return = checker.expected_return.replace(*ret_ty);
    for (cap_name, cap_ty) in &cap_tys {
        let _ = checker.declare(
            cap_name.clone(),
            LocalInfo {
                ty: cap_ty.clone(),
                mutable: true,
                moved: false,
                borrowed: false,
                declared_class: None,
            },
        );
    }
    for (p, ty) in real_params.iter().zip(param_tys.iter()) {
        let _ = checker.declare(
            p.name.clone(),
            LocalInfo {
                ty: ty.clone(),
                mutable: true,
                moved: false,
                borrowed: false,
                declared_class: None,
            },
        );
    }
    for s in &body {
        checker.check_stmt(ast, &s.clone());
    }
    checker.expected_return = saved_return;
    checker.scopes = saved_scopes;
    Ok(user_fn_ty)
}
