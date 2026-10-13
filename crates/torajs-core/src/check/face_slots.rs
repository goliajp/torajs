//! The checker's half of [`crate::ast::face_slots`]: the admit points
//! of a property slot (a field store, an object literal's field against
//! a declared struct type) and of a return slot record the slot when the
//! stored function value has another face than the slot's type, as the
//! `let` lane does for a binding (`check_stmt_let_decl::fn_face_slot`).
//!
//! A binding with no annotation initialized by a call of a function in
//! the return census (`const h = ret()`) holds that value too, so it
//! joins the binding census. That verdict reads the census while the
//! program is still being checked, as does the typing of a call through
//! a census slot; a store checked after such a read enters the census
//! too late for it, so the program is checked again with the census
//! seeded (`check_entry::fixpoint_with`). Only a key some read asked
//! for before its store marks the run [`Reads::stale`].

use std::collections::HashSet;

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

/// What the reads of one run asked the census for and did not find: a
/// field by its name alone (whatever the receiver), a function by its
/// name. Every call through a field asks, so the question allocates
/// nothing once its name is on record.
#[derive(Debug, Default)]
pub(crate) struct Reads {
    fields: HashSet<String>,
    rets: HashSet<String>,
    /// bindings, by declaration, a call reached before any store
    /// joined them to the census
    lets: HashSet<ExprId>,
    /// parameters, by function, a call reached while the body was
    /// checked before any call joined them to the census
    params: HashSet<(String, String)>,
    /// a store entered a key some read had already asked for
    pub(crate) stale: bool,
}

fn remember(set: &mut HashSet<String>, name: &str) {
    if !set.contains(name) {
        set.insert(name.to_string());
    }
}

/// The census owner of receiver `obj` when its field `name` holds such
/// a value ([`crate::ast::face_slots::FaceSlots::field_hit`]); a miss
/// is remembered.
pub(crate) fn field_read(
    checker: &mut Checker,
    ast: &Ast,
    obj: ExprId,
    name: &str,
) -> Option<String> {
    let recv = checker.expr_types.get(&obj)?;
    let census = &checker.face_slots;
    let hit = (census.fields.iter().any(|(_, n)| n == name))
        .then(|| owner_key(ast, recv))
        .flatten()
        .filter(|owner| census.field_hit(&ast.class_parents, owner, name));
    if hit.is_none() && matches!(recv, Type::ClassRef(_) | Type::Struct(_)) {
        remember(&mut checker.face_slot_reads.fields, name);
    }
    hit
}

/// Does function `g` return such a value? A miss is remembered.
pub(crate) fn ret_read(checker: &mut Checker, g: &str) -> bool {
    let hit = checker.face_slots.rets.contains(g);
    if !hit {
        remember(&mut checker.face_slot_reads.rets, g);
    }
    hit
}

/// Did a store join the binding declared by `init` to the census
/// (`n = raw` with `raw: any`)? A miss is remembered.
pub(crate) fn let_read(checker: &mut Checker, init: ExprId) -> bool {
    let hit = checker.face_mismatch_lets.contains(&init);
    if !hit {
        checker.face_slot_reads.lets.insert(init);
    }
    hit
}

/// A store joins the binding declared by `init` to the census; a call
/// typed before it is typed again in the next run.
pub(crate) fn record_let_store(checker: &mut Checker, init: ExprId) {
    if checker.face_mismatch_lets.insert(init) {
        checker.face_slot_reads.stale |= checker.face_slot_reads.lets.contains(&init);
    }
}

/// Did a call hand parameter `p` of `owner` a value of another face
/// (`obj.m(mkS(1))` into `m(f: (y?: number) => number)`)? A
/// specialization asks for its generic's verdict too. A miss is
/// remembered.
pub(crate) fn param_read(checker: &mut Checker, owner: &str, p: &str) -> bool {
    let generic = crate::ast::boxed_params::spec_generic(owner);
    let hit = [Some(owner), generic].into_iter().flatten().any(|f| {
        let key = (f.to_string(), p.to_string());
        checker.face_mismatch_params.contains(&key)
    });
    if !hit {
        checker
            .face_slot_reads
            .params
            .insert((owner.to_string(), p.to_string()));
    }
    hit
}

/// A call or a store joins parameter `p` of `owner` to the census; a
/// call through it typed before then (the body checked ahead of the
/// call, as an object literal's method is) is typed again in the next
/// run.
pub(crate) fn record_param(checker: &mut Checker, owner: &str, p: &str) {
    let key = (owner.to_string(), p.to_string());
    if checker.face_mismatch_params.insert(key.clone()) {
        let reads = &checker.face_slot_reads.params;
        checker.face_slot_reads.stale |= reads.contains(&key)
            || reads
                .iter()
                .any(|(f, q)| q == p && crate::ast::boxed_params::spec_generic(f) == Some(owner));
    }
}

/// Do the two signatures differ in a way the calling convention sees?
/// The comparison `check_type_of_call::fn_face::face_ann` makes, without
/// an annotation to re-spell: a parameter past the slot's arity is only
/// handed undefined, and a return the slot discards or the value never
/// produces is never read. An `any` value may be a function of any
/// face, so it differs unless the slot passes no argument and discards
/// the return; a rest-tail slot differs from none, as its calls take
/// the boxed entry anyway.
pub(crate) fn faces_differ(value_ty: &Type, slot_ty: &Type) -> bool {
    let slot_ty = match slot_ty {
        Type::Nullable(inner) => inner.as_ref(),
        other => other,
    };
    let Type::Function(fps, fret) = slot_ty else {
        return false;
    };
    // a rest-tail slot's calls ride the boxed entry already
    if matches!(fps.last(), Some(Type::Rest(_))) {
        return false;
    }
    let discards = matches!(**fret, Type::Void | Type::Undefined);
    let (aps, aret) = match value_ty {
        Type::Function(aps, aret) => (aps, aret),
        Type::Any => return !(fps.is_empty() && discards),
        _ => return false,
    };
    if matches!(aps.last(), Some(Type::Rest(_))) {
        return true;
    }
    let params = aps.iter().zip(fps).any(|(a, f)| a != f);
    let ret = !(aret == fret || discards || matches!(**aret, Type::Never));
    params || ret
}

/// The census owner of a receiver typed `ty`: a class instance's class,
/// or an inline struct type's structural spelling.
pub(crate) fn owner_key(ast: &Ast, ty: &Type) -> Option<String> {
    match ty {
        Type::ClassRef(n) if ast.class_parents.contains_key(n) => Some(n.clone()),
        Type::Struct(_) => Some(crate::check::type_to_ann(ty)),
        _ => None,
    }
}

/// A store of `value_ty` into field `field` of a receiver typed `recv`,
/// the field declared `slot_ty`.
pub(crate) fn record_field_store(
    checker: &mut Checker,
    ast: &Ast,
    recv: &Type,
    field: &str,
    slot_ty: &Type,
    value_ty: &Type,
) {
    if !faces_differ(value_ty, slot_ty) {
        return;
    }
    let Some(mut owner) = owner_key(ast, recv) else {
        return;
    };
    // the class that declares the field, which every receiver that can
    // hold it names among its ancestors
    while let Some(Some(parent)) = ast.class_parents.get(&owner) {
        let declares = matches!(
            checker.class_structs.get(parent),
            Some(Type::Struct(fs)) if fs.iter().any(|(n, _)| n == field)
        );
        if !declares {
            break;
        }
        owner = parent.clone();
    }
    let key = (owner, field.to_string());
    if !checker.face_slots.fields.contains(&key) {
        checker.face_slot_reads.stale |= checker.face_slot_reads.fields.contains(field);
        checker.face_slots.fields.insert(key);
    }
}

/// The fields of the object literal `value` admitted into a slot typed
/// `slot_ty`.
pub(crate) fn record_objlit_stores(
    checker: &mut Checker,
    ast: &Ast,
    slot_ty: &Type,
    value: ExprId,
) {
    let Expr::ObjectLit { fields } = ast.get_expr(value) else {
        return;
    };
    let Type::Struct(slot_fields) = slot_ty else {
        return;
    };
    for (key, v) in fields {
        let Some((_, field_ty)) = slot_fields.iter().find(|(n, _)| n == key) else {
            continue;
        };
        let Some(value_ty) = checker.expr_types.get(v).cloned() else {
            continue;
        };
        record_field_store(checker, ast, slot_ty, &key.lossy(), field_ty, &value_ty);
    }
}

/// `return value` (typed `value_ty`) from the function being checked,
/// which declares return `ret_ty`.
pub(crate) fn record_return(
    checker: &mut Checker,
    ast: &Ast,
    ret_ty: &Type,
    value: ExprId,
    value_ty: &Type,
) {
    record_objlit_stores(checker, ast, ret_ty, value);
    if faces_differ(value_ty, ret_ty)
        && let Some(owner) = checker.ret_owner.clone()
    {
        record_ret(checker, owner);
    }
}

fn record_ret(checker: &mut Checker, owner: String) {
    if !checker.face_slots.rets.contains(&owner) {
        checker.face_slot_reads.stale |= checker.face_slot_reads.rets.contains(&owner);
        checker.face_slots.rets.insert(owner);
    }
}

/// The binding `name` with no annotation initialized by `init` (typed
/// `init_ty`). A `const` holding a closure of the return census is
/// called as that closure (`const r = (): T => mk2(1); r()()`), so the
/// census holds its name too. A binding initialized by a call of a
/// function in the census holds what that returns: it joins the binding
/// census, and the answer says so.
pub(crate) fn ret_slot_let(
    checker: &mut Checker,
    ast: &Ast,
    (name, mutable): (&str, bool),
    init: ExprId,
    init_ty: &Type,
) -> bool {
    match ast.get_expr(init) {
        Expr::Closure { fn_name, .. } if !mutable && checker.face_slots.rets.contains(fn_name) => {
            record_ret(checker, name.to_string());
            false
        }
        Expr::Call { callee, .. }
            if matches!(init_ty, Type::Function(..))
                && matches!(ast.get_expr(*callee), Expr::Ident(g) if ret_read(checker, g)) =>
        {
            checker.face_mismatch_lets.insert(init);
            true
        }
        _ => false,
    }
}
