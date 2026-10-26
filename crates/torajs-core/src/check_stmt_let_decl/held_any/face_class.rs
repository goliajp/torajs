//! The TypeScript rules for a held `any` value read through a binding
//! declared with a class or a function type.
//!
//! Such a binding types every read `any` (`check_type_of_ident`), as
//! its members are read off the value it holds, but its declaration
//! still says what the program may do with it, exactly as a typed
//! receiver's does: `q.m("x")` with `m(s: number)`, `q.n = "s"` with
//! `n: number`, `g("x")` with `g: (x: number) => number` are refused
//! whatever the value is at run time, as tsc refuses them. Each check
//! here answers what the typed receiver's own path answers, and no
//! more: a member the typed path admits is admitted.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type, resolve_class_ref};
use crate::check_assignable::is_assignable_to_resolved;

use super::face::face;

/// The type member `name` of a value of the declared type `recv` has
/// for the TypeScript rules; `None` when it has none to tell.
pub(crate) fn member_face(c: &Checker, ast: &Ast, recv: &Type, name: &str) -> Option<Type> {
    match recv {
        Type::ClassRef(cls) => class_member(c, ast, cls, name),
        Type::Number | Type::String | Type::Boolean | Type::Function(..) => {
            crate::check_type_of_member::try_family_dispatch(recv, name)?.ok()
        }
        _ => None,
    }
}

/// The type of member `name` of an instance of `cls` as declared: a
/// field, a method (its own parameters, the receiver dropped), or a
/// getter's return. A generic class or member has no single answer.
fn class_member(c: &Checker, ast: &Ast, cls: &str, name: &str) -> Option<Type> {
    if !c.class_structs.contains_key(cls) {
        return None;
    }
    let shape = resolve_class_ref(
        &Type::ClassRef(cls.to_string()),
        &c.class_structs,
        &c.aliases,
        &c.generic_alias_decls,
    );
    if let Type::Struct(fields) = &shape
        && let Some((_, t)) = fields.iter().find(|(f, _)| f == name)
    {
        return Some(t.clone());
    }
    let mut cur = Some(cls.to_string());
    while let Some(k) = cur {
        let m = format!("__cm_{k}__{name}");
        if c.globals.contains_key(&m) {
            return method_sig(c, ast, &m);
        }
        cur = ast.class_parents.get(&k).cloned().flatten();
    }
    let hit = crate::ast::accessor_lookup::accessor_getter_in_chain(ast, cls, name)?;
    match c.globals.get(&hit.fn_name) {
        Some(Type::Function(_, ret)) if !c.generic_type_params.contains_key(&hit.fn_name) => {
            Some((**ret).clone())
        }
        _ => None,
    }
}

/// A member `name` read through `obj`, whose [`face`] is a class or a
/// function type: refused where a receiver typed so is refused (a
/// visibility the class does not grant, a private name it does not
/// declare, a member a function value does not have).
pub(crate) fn check_receiver(
    c: &Checker,
    ast: &Ast,
    (obj, name): (ExprId, &str),
    recv: &Type,
    lenient_missing: bool,
) -> Result<(), String> {
    let found = match recv {
        Type::ClassRef(cls) => {
            crate::check_type_of_member_accessor::enforce_visibility(c, ast, &obj, name)?;
            class_member(c, ast, cls, name).is_some()
                || crate::ast::accessor_lookup::declared_setter_in_chain(ast, cls, name).is_some()
        }
        Type::Function(..) => match crate::check_type_of_member::try_family_dispatch(recv, name) {
            Some(r) => return r.map(|_| ()),
            None => false,
        },
        _ => return Ok(()),
    };
    if found {
        return Ok(());
    }
    crate::check_type_of_member_terminal_miss::answer(
        ast,
        &obj,
        name,
        recv,
        &crate::check_type_of_member_terminal_miss::MissFacts {
            lenient_missing,
            spec_body: c.checking_spec_body,
            declared_private: c.declared_privates.contains(name),
        },
    )
    .map(|_| ())
}

/// The type a value has for the TypeScript rules: its [`face`], or
/// what the checker typed it.
fn shown(c: &Checker, ast: &Ast, eid: ExprId) -> Option<Type> {
    face(c, ast, eid).or_else(|| c.expr_types.get(&eid).cloned())
}

fn fits(c: &Checker, slot: &Type, value: &Type) -> bool {
    is_assignable_to_resolved(
        slot,
        value,
        &c.class_structs,
        &c.aliases,
        &c.generic_alias_decls,
    )
}

/// The type the callee of a call has for the TypeScript rules, and the
/// arguments its parameters take: a held function binding, a method
/// read through a held class binding, or the static call desugar made
/// of such a method call (`__cm_<C>__<m>(q, …)` or the chain's
/// `__dispatch_<m>(q, …)`, the receiver first).
pub(crate) fn callee_face<'a>(
    c: &Checker,
    ast: &Ast,
    callee: ExprId,
    args: &'a [ExprId],
) -> Option<(Type, &'a [ExprId])> {
    match ast.get_expr(callee) {
        Expr::Member { obj, name } => {
            Some((member_face(c, ast, &face(c, ast, *obj)?, name)?, args))
        }
        Expr::Ident(f) if f.starts_with("__cm_") || f.starts_with("__dispatch_") => {
            let (recv, rest) = args.split_first()?;
            let Type::ClassRef(cls) = face(c, ast, *recv)? else {
                return None;
            };
            let sig = match f.strip_prefix("__dispatch_") {
                Some(m) => class_member(c, ast, &cls, m)?,
                None => method_sig(c, ast, f)?,
            };
            Some((sig, rest))
        }
        _ => Some((face(c, ast, callee)?, args)),
    }
}

/// The signature of the class method `m` (`__cm_<C>__<name>`) as a
/// call through an instance spells it: the receiver dropped.
fn method_sig(c: &Checker, ast: &Ast, m: &str) -> Option<Type> {
    let Some(Type::Function(params, ret)) = c.globals.get(m) else {
        return None;
    };
    if c.generic_type_params.contains_key(m) {
        return None;
    }
    let skip = 1 + usize::from(ast.method_argv_fns.contains(m));
    Some(Type::Function(params.get(skip..)?.to_vec(), ret.clone()))
}

/// A call through `callee`, a held function binding or a method read
/// through a held class binding: each argument against its parameter,
/// and a missing one where the parameter does not take `undefined`, as
/// a typed callee's call is checked. Extra arguments are dropped there
/// and here alike; a rest parameter ends the check.
pub(crate) fn check_call(
    c: &mut Checker,
    ast: &Ast,
    callee: ExprId,
    args: &[ExprId],
) -> Result<(), String> {
    if !c.held_any.objects {
        return Ok(());
    }
    // a route may answer a method call off the callee's syntax without
    // reading the receiver: a held one is typed here, which records
    // its face
    if let Expr::Member { obj, .. } = ast.get_expr(callee)
        && let Expr::Ident(n) = ast.get_expr(*obj)
        && !c.expr_types.contains_key(obj)
        && (c.lookup(n).map(|i| i.held_any)).unwrap_or_else(|| c.held_any.globals.contains(n))
    {
        let _ = c.type_of(ast, *obj);
    }
    let c = &*c;
    let Some((Type::Function(params, _), args)) = callee_face(c, ast, callee, args) else {
        return Ok(());
    };
    for (i, p) in params.iter().enumerate() {
        if matches!(p, Type::Rest(_)) || format!("{p:?}").contains("TypeVar(") {
            return Ok(());
        }
        let Some(&arg) = args.get(i) else {
            if fits(c, p, &Type::Undefined) {
                continue;
            }
            return Err(format!(
                "expected {} argument(s), got {}",
                params.len(),
                args.len()
            ));
        };
        if let Some(t) = shown(c, ast, arg)
            && !fits(c, p, &t)
        {
            return Err(format!("argument {i}: expected {p:?}, got {t:?}"));
        }
    }
    Ok(())
}

/// A store of `value` into member `field` through `obj`, whose [`face`]
/// is a class type: held to the field's declared type, or to the
/// setter's parameter, as a typed receiver's store is.
pub(crate) fn check_store(
    c: &Checker,
    ast: &Ast,
    obj: ExprId,
    field: &str,
    value: ExprId,
) -> Result<(), String> {
    if c.held_any.reads.is_empty() {
        return Ok(());
    }
    let Some(Type::ClassRef(cls)) = face(c, ast, obj) else {
        return Ok(());
    };
    let Some(t) = shown(c, ast, value) else {
        return Ok(());
    };
    let shape = resolve_class_ref(
        &Type::ClassRef(cls.clone()),
        &c.class_structs,
        &c.aliases,
        &c.generic_alias_decls,
    );
    if let Type::Struct(fields) = &shape
        && let Some((_, slot)) = fields.iter().find(|(f, _)| f == field)
    {
        if fits(c, slot, &t) {
            return Ok(());
        }
        return Err(format!(
            "type mismatch assigning to `{field}`: field is {slot:?}, value is {t:?}"
        ));
    }
    match crate::check_assign_target::setter_param(c, ast, &cls, field) {
        Some(p) if !fits(c, &p, &t) => Err(format!(
            "type mismatch assigning to accessor `{cls}.{field}`: setter expects {p:?}, value is {t:?}"
        )),
        _ => Ok(()),
    }
}
