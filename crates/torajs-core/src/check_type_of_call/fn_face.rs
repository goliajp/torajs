//! A function value handed to a fn-typed parameter whose face differs
//! from the value's own: `caller(mk(1))` where `caller(f: (y?: number)
//! => number)` and `mk` returns `(y?: number) => any`.
//!
//! The callback admit (`check_type_of_call_callback_subtype`) accepts
//! the value, as TS does: an optional or `any` parameter takes every
//! argument the formal passes, and an `any` return is assignable to a
//! typed one. The two faces lower to different calling conventions,
//! though — an optional `number` slot is a NaN-boxed value where a
//! plain one is an f64, and so is an `any` return against a `number`
//! one — and a call through the parameter is shaped by the PARAMETER's
//! face. Handing the value over unchanged makes the callee read a box
//! as an f64 (NaN, SIGBUS).
//!
//! Wrapping the value in an adapter would change which function
//! object the callee holds (§7.2.10 SameValue: `f === g`, `.name`, a
//! Map key), and routing every call through the boxed entry taxes the
//! `reduce(xs, f)` shape with four extra calls per iteration. So the
//! CALLEE moves instead: the call site is recorded as an any-widen site
//! whose target is the value's face ([`WidenTarget::Face`]), and
//! `check_monomorph_any_widen` clones the callee with that parameter
//! spelled in the value's face. The clone's calls through the param
//! then use the value's own convention; the original stays untouched
//! for every call whose faces agree. The type flows: a value that
//! reaches the call through `id(x)` carries the same face.
//!
//! Only the direction the admit opens is cloned: the value's slot is
//! `any`, or the optional spelling of the formal's; the value's return
//! is `any` / `never`, or a scalar against an `any` formal return. A
//! value of another arity is spelled as it is (the clone's calls pass
//! what the formal passes; extra arguments drop, missing ones bind
//! undefined). A value whose parameter is NARROWER than the formal
//! (`(s: string)` into `(s: any)`) is the boxed-entry census's
//! (`argv_boxed_params`): its adapter tests each argument against the
//! body's own type. So is a parameter the clone cannot serve — of a
//! closure a function-local binding holds, or in a face this lane does
//! not spell (`Checker::face_mismatch_params`), and a parameter of a
//! method called through a member expression: an object literal's
//! that a binding holds, or a class's and each override's.

use super::any_widen::{clonable_callee, record_widen_site};
use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::{Checker, FnSlot, Type, WidenTarget};

mod handed_on;
pub(crate) use handed_on::{box_census_arg, held_face};

/// Record the call `eid` as a face-widen site for argument `i` when the
/// callback admit holds between `param_ty` and `arg_ty` and the callee
/// is a clonable declaration. Returns whether the argument is admitted.
#[allow(clippy::too_many_arguments)]
pub(in crate::check_type_of_call) fn fn_face_into_fn_param(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: &ExprId,
    arg_ty: &Type,
    param_ty: &Type,
    i: usize,
) -> bool {
    let (Type::Function(aps, aret), Type::Function(fps, fret)) = (arg_ty, param_ty) else {
        return false;
    };
    if !crate::check_type_of_call_callback_subtype::matches_resolved(
        param_ty,
        arg_ty,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    ) {
        return false;
    }
    if aps.iter().chain(fps).any(|t| matches!(t, Type::Rest(_))) {
        return true;
    }
    let n = match ast.get_expr(*callee) {
        Expr::Ident(n) => n,
        Expr::Member { obj, name } => {
            let mut bodies = class_method_decls(checker, ast, *obj, name);
            bodies.extend(objlit_method_decl(checker, ast, *obj, name));
            for body in bodies {
                // the receiver binds the body's `__this`, not an argument
                let at = i + usize::from(receiver_param(ast, &body));
                box_param(checker, ast, &body, at, arg_ty, param_ty);
            }
            return true;
        }
        _ => return true,
    };
    let Some(name) = clonable_callee(checker, ast, n, i) else {
        if let Some(name) = local_closure_decl(checker, ast, n) {
            box_param(checker, ast, &name, i, arg_ty, param_ty);
        }
        return true;
    };
    // a class method called directly (`k.m(g)` with no override in
    // sight, the receiver passed first) is not served by a clone
    if name.starts_with("__cm_") {
        box_param(checker, ast, &name, i, arg_ty, param_ty);
        return true;
    }
    let Some((pname, pann)) = user_param(ast, &name, i) else {
        return true;
    };
    if ast
        .argv_boxed_params
        .get(&name)
        .is_some_and(|set| set.contains(&pname))
    {
        return true;
    }
    match face_ann(&pann, aps, aret, fps, fret) {
        Face::Same => {}
        Face::Spelled(ann) => record_widen_site(
            checker,
            eid,
            &name,
            i,
            WidenTarget::Face {
                ann,
                ty: arg_ty.clone(),
            },
        ),
        Face::Unspelled => box_param(checker, ast, &name, i, arg_ty, param_ty),
    }
    true
}

/// A fn-typed parameter of a generic that mentions none of its type
/// variables (`type_params`), handed a function value of another face
/// (`pass<T>(x: T, f: (y?: number) => number)` called with `mk2(1)`):
/// the callback admit takes it as it does for a plain callee, and the
/// argument goes to `faced`, which [`box_generic_params`] enters in the
/// boxed-entry census once the whole call matches.
pub(crate) fn fn_face_into_generic_param(
    checker: &Checker,
    type_params: &[String],
    (i, arg_ty, param_ty): (usize, &Type, &Type),
    faced: &mut Vec<(usize, Type, Type)>,
) -> bool {
    let admitted = matches!((arg_ty, param_ty), (Type::Function(..), Type::Function(..)))
        && !type_params
            .iter()
            .any(|tp| crate::check_typevar::typevar_appears_in(param_ty, tp))
        && crate::check_type_of_call_callback_subtype::matches_resolved(
            param_ty,
            arg_ty,
            &checker.class_structs,
            &checker.aliases,
            &checker.generic_alias_decls,
        );
    if admitted {
        faced.push((i, arg_ty.clone(), param_ty.clone()));
    }
    admitted
}

/// The parameters of the generic `name` an admitted value of another
/// face reached join the boxed-entry census, whose adapter answers each
/// value by its own face in every specialization (the face clone lane
/// does not reach a generic).
pub(crate) fn box_generic_params(
    checker: &mut Checker,
    ast: &Ast,
    name: &str,
    faced: &[(usize, Type, Type)],
) {
    for (i, arg_ty, param_ty) in faced {
        if let (Type::Function(aps, _), Type::Function(fps, _)) = (arg_ty, param_ty)
            && !aps.iter().chain(fps).any(|t| matches!(t, Type::Rest(_)))
        {
            box_param(checker, ast, name, *i, arg_ty, param_ty);
        }
    }
}

/// The lifted body an immutable function-local closure binding `n`
/// holds: the clone lane cannot stand in for it, but its declaration
/// is in sight.
fn local_closure_decl(checker: &Checker, ast: &Ast, n: &str) -> Option<String> {
    let info = checker.lookup(n)?;
    let Some(FnSlot::Let(init)) = info.decl.filter(|_| !info.mutable) else {
        return None;
    };
    match ast.get_expr(init) {
        Expr::Closure { fn_name, .. } => Some(fn_name.clone()),
        _ => None,
    }
}

/// The lifted body of method `key` on the object literal an immutable
/// binding `recv` holds (`const obj = { m(f) { … } }; obj.m(g)`): the
/// clone lane resolves callees by name and cannot stand in for it, but
/// the body is in sight. A binding the scopes do not hold is read as
/// the top-level declaration a named body reaches.
fn objlit_method_decl(checker: &Checker, ast: &Ast, recv: ExprId, key: &str) -> Option<String> {
    let Expr::Ident(n) = ast.get_expr(recv) else {
        return None;
    };
    let init = match checker.lookup(n) {
        Some(info) => match info.decl {
            Some(FnSlot::Let(init)) if !info.mutable => init,
            _ => return None,
        },
        None => {
            let (init, _) = crate::ast_refs::unique_toplevel_binding(ast, n)?;
            let immutable = crate::ast::toplevel_stmts_flat(ast)
                .into_iter()
                .any(|s| matches!(s, Stmt::LetDecl { init: i, mutable: false, .. } if *i == init));
            if !immutable {
                return None;
            }
            init
        }
    };
    let Expr::ObjectLit { fields } = ast.get_expr(init) else {
        return None;
    };
    // a later duplicate key wins (§13.2.5.5), so the last one is the method
    let (_, v) = fields.iter().rev().find(|(k, _)| k.lossy() == key)?;
    match ast.get_expr(*v) {
        Expr::Closure { fn_name, .. } => Some(fn_name.clone()),
        _ => None,
    }
}

/// Whether the body `name` binds a receiver ahead of its arguments.
fn receiver_param(ast: &Ast, name: &str) -> bool {
    ast.stmts.iter().any(|s| {
        matches!(s, Stmt::FnDecl { name: n, params, .. } if n == name
            && params.iter().find(|p| p.name != "__env").is_some_and(|p| p.name == "__this"))
    })
}

/// Every body a call of instance method `key` on `recv` can run: the
/// one the receiver's class resolves (its own or the nearest
/// ancestor's), and each override in a class derived from it — the
/// receiver may be an instance of any of them.
fn class_method_decls(checker: &Checker, ast: &Ast, recv: ExprId, key: &str) -> Vec<String> {
    let Some(cname) = checker
        .expr_types
        .get(&recv)
        .and_then(|t| crate::check_type_of_member_accessor::class_name_of(t, ast))
    else {
        return Vec::new();
    };
    let has = |c: &str| {
        let body = format!("__cm_{c}__{key}");
        ast.stmts
            .iter()
            .any(|s| matches!(s, Stmt::FnDecl { name, .. } if *name == body))
            .then_some(body)
    };
    let mut out: Vec<String> = Vec::new();
    let mut up = Some(cname.clone());
    while let Some(c) = up {
        if let Some(b) = has(&c) {
            out.push(b);
            break;
        }
        up = ast.class_parents.get(&c).cloned().flatten();
    }
    let descends = |c: &String| {
        let mut c = c;
        while let Some(Some(p)) = ast.class_parents.get(c) {
            if *p == cname {
                return true;
            }
            c = p;
        }
        false
    };
    out.extend(
        ast.class_parents
            .keys()
            .filter(|c| descends(c))
            .filter_map(|c| has(c)),
    );
    out
}

/// The callee `name` cannot take the value's face as a clone: its
/// param `i` joins the boxed-entry census instead, whose adapter
/// unboxes per each value's own face.
fn box_param(
    checker: &mut Checker,
    ast: &Ast,
    name: &str,
    i: usize,
    arg_ty: &Type,
    param_ty: &Type,
) {
    let Some((pname, pann)) = user_param(ast, name, i) else {
        return;
    };
    if let_face_ann(&pann, arg_ty, param_ty) != Face::Same {
        crate::check::face_slots::record_param(checker, name, &pname);
    }
}

/// The name and annotation of user param `i` of the declaration `name`
/// — past a lifted body's `__env` head and a hidden argv slot, the way
/// `any_widen::param_has_default` counts them.
fn user_param(ast: &Ast, name: &str, i: usize) -> Option<(String, String)> {
    ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name: n, params, ..
        } if n == name => {
            let user = &params[usize::from(params.first().is_some_and(|p| p.name == "__env"))..];
            let hidden = crate::check_stmt_fn_decl::argv_slot_hidden(name, user);
            user.iter()
                .filter(|p| !(hidden && p.name == "__torajs_argv"))
                .nth(i)
                .and_then(|p| Some((p.name.clone(), p.type_ann.clone()?)))
        }
        _ => None,
    })
}

/// How a function value's face stands against the fn-typed slot it
/// reaches.
#[derive(Debug, PartialEq)]
pub(crate) enum Face {
    /// Nothing differs in a way the calling convention sees.
    Same,
    /// The slot's annotation re-spelled in the value's face.
    Spelled(String),
    /// The faces differ in a way this lane does not spell.
    Unspelled,
}

/// The param's annotation re-spelled in the value's face: the same
/// repr marker (`__fn` / `__cls`, decided by the closure-param tagger
/// from what flows into the slot), each slot the formal's own spelling
/// where the two agree and the value's where they differ, the return
/// likewise. A `never` return is never read, so it differs from no
/// other return.
pub(crate) fn face_ann(pann: &str, aps: &[Type], aret: &Type, fps: &[Type], fret: &Type) -> Face {
    let t = pann.trim();
    let marker = if t.starts_with("__cls(") {
        "__cls"
    } else if t.starts_with("__fn(") {
        "__fn"
    } else {
        return Face::Unspelled;
    };
    let Some(canon) = crate::num_width::fn_type_canon(t) else {
        return Face::Unspelled;
    };
    let Some((slots, ret)) = crate::num_width::split_fn_type(&canon) else {
        return Face::Unspelled;
    };
    let slots: Vec<&str> = slots.into_iter().filter(|p| !p.is_empty()).collect();
    if slots.len() != fps.len() {
        return Face::Unspelled;
    }
    let mut changed = false;
    let mut out: Vec<String> = Vec::with_capacity(aps.len());
    for (j, a) in aps.iter().enumerate() {
        // a slot past the formal's arity is only ever handed undefined,
        // which the admit limited to `any` / optional; it is spelled but
        // does not call for a clone on its own (the env-first ABI binds
        // an unpassed slot undefined already)
        let Some(f) = fps.get(j) else {
            out.push(match a {
                Type::Nullable(inner) => {
                    format!("__nullable({})", crate::check::type_to_ann(inner))
                }
                other => crate::check::type_to_ann(other),
            });
            continue;
        };
        let spelled = slots[j];
        if a == f {
            out.push(spelled.to_string());
            continue;
        }
        changed = true;
        match a {
            Type::Any => out.push("any".to_string()),
            Type::Nullable(inner) if **inner == *f => {
                out.push(format!("__nullable({spelled})"));
            }
            _ => return Face::Unspelled,
        }
    }
    let ret = match (aret, fret) {
        (a, f) if a == f => ret.to_string(),
        // the formal discards the value, or the value never returns:
        // the register read is never used
        (_, Type::Void | Type::Undefined) | (Type::Never, _) => ret.to_string(),
        (Type::Any, _)
        | (Type::Number | Type::String | Type::Boolean | Type::BigInt, Type::Any) => {
            changed = true;
            crate::check::type_to_ann(aret)
        }
        _ => return Face::Unspelled,
    };
    if !changed {
        return Face::Same;
    }
    Face::Spelled(crate::type_ann_fnsig::fn_type_ann(
        marker,
        &out.join("|"),
        &ret,
    ))
}

/// How a fn-typed binding's value of type `value_ty` stands against
/// the binding's annotation `ann` (resolved: `ann_ty`): [`face_ann`]
/// over the two signatures. `Spelled` is the annotation an immutable
/// binding takes, so its calls are shaped by what it holds at no
/// run-time cost.
pub(crate) fn let_face_ann(ann: &str, value_ty: &Type, ann_ty: &Type) -> Face {
    let (Type::Function(aps, aret), Type::Function(fps, fret)) = (value_ty, ann_ty) else {
        return Face::Same;
    };
    if aps.iter().chain(fps).any(|t| matches!(t, Type::Rest(_))) {
        return Face::Unspelled;
    }
    face_ann(ann, aps, aret, fps, fret)
}
