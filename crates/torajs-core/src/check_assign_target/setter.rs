//! Accessor-setter resolution for the Member-target write checker —
//! `try_objlit_setter` (layout `__setter_<f>` field on the literal's
//! own type) and `try_accessor_setter` (nominal class chain lookup
//! with generic substitution). Split from `check_assign_target.rs`
//! (rotation 441 — the 3a resolve_class_ref + dynamic-number-key
//! admit pushed the parent past the 500-line cap; the r413 watch
//! named this exact family as the cut). Bodies verbatim; the parent
//! keeps the member/index dispatch these two answer into.

use super::*;
use crate::ast::PropKey;

/// RFC 20260714-objlit-accessor blade 2 — `o.b = v` where the literal
/// declared `set b(v) { ... }`. The setter closure is a layout field
/// (`__setter_b`), so this reads straight off the receiver's own type —
/// no reverse lookup, unlike the class lane above, whose scan of
/// `aliases` for a structurally-equal entry is what lets a plain `{a:1}`
/// reach a same-layout class's accessor (RFC §2.1).
///
/// Assigning to a getter-only accessor is rejected here rather than
/// silently writing a data field that doesn't exist.
pub(super) fn try_objlit_setter(
    checker: &mut Checker,
    ast: &Ast,
    fields: &[(PropKey, Type)],
    field: &str,
    value: ExprId,
) -> Result<Option<Type>, String> {
    // The probe names build ONCE — a per-comparison `format!` inside
    // the scan closure allocated O(fields) strings per assignment,
    // O(N²) across a wide-struct program (21s checker stall on the
    // 75KB test262 unicode-ident class file, rotation 268 profile).
    let setter_name = format!("__setter_{field}");
    let setter = fields.iter().find(|(n, _)| *n == setter_name);
    let Some((setter_key, setter_ty @ Type::Function(..))) = setter else {
        let getter_name = format!("__getter_{field}");
        if fields.iter().any(|(n, _)| *n == getter_name) {
            return Err(format!(
                "cannot assign to `{field}`: it is a getter-only accessor"
            ));
        }
        return Ok(None);
    };
    let setter_param = crate::check_type_of_object_lit::setter_value_ty;
    let Some(param_ty) = setter_param((&*checker, ast), fields, setter_key, setter_ty) else {
        return Err(format!("setter `{field}` declares no parameter"));
    };
    let value_ty = checker.type_of(ast, value)?;
    if !is_assignable_to_resolved(
        &param_ty,
        &value_ty,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    ) {
        return Err(format!(
            "type mismatch assigning to accessor `{field}`: setter expects {param_ty:?}, value is {value_ty:?}"
        ));
    }
    Ok(Some(param_ty))
}

/// `q.X = v` through a class setter. The value param follows the
/// receiver and, when the body reads `arguments` (the method argv face),
/// the synthetic `__torajs_argv` that the boxed adapter and the direct
/// accessor lanes fill; no caller spells it.
pub(super) fn try_accessor_setter(
    checker: &mut Checker,
    ast: &Ast,
    site: ExprId,
    obj_ty: &Type,
    field: &str,
    value: ExprId,
) -> Result<Option<Type>, String> {
    // RFC 20260715-nominal-class-identity — the setter's class comes
    // from the receiver's NAME. Scanning `aliases` for a class with the
    // receiver's struct shape let a plain `{a: 1}` write through
    // `class C { a; set b(v) }`'s setter.
    let cls = match obj_ty {
        Type::ClassRef(n)
            if ast
                .class_parents
                .contains_key(n.split('<').next().unwrap_or(n)) =>
        {
            n.clone()
        }
        _ => return Ok(None),
    };
    // Blade 2 (rotation 413) — the pair may live on an ANCESTOR
    // (§10.1.9 walks the prototype chain). A subclass redeclaring the
    // property sends the write to the any-member lane (`per_instance`
    // false), but the value is still held to the setter the static
    // type declares: refusing it is the type error whichever instance
    // arrives.
    use crate::ast::accessor_lookup::{accessor_setter_in_chain, declared_setter_in_chain};
    let (hit, per_instance) = match accessor_setter_in_chain(ast, &cls, field) {
        Some(hit) => (hit, true),
        None => match declared_setter_in_chain(ast, &cls, field) {
            Some(hit) => (hit, false),
            None => return Ok(None),
        },
    };
    let Some((setter_param_ty, type_args)) = setter_value_ty(checker, ast, &hit) else {
        return Ok(None);
    };
    // Record the write site so the mono pass emits the setter
    // specialization and the lowering retargets its direct call.
    if per_instance && let Some(type_args) = type_args {
        checker
            .generic_call_sites
            .insert(site, (hit.fn_name.clone(), type_args));
    }
    let value_ty = checker.type_of(ast, value)?;
    if !is_assignable_to_resolved(
        &setter_param_ty,
        &value_ty,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    ) {
        return Err(format!(
            "type mismatch assigning to accessor `{cls}.{field}`: setter expects {setter_param_ty:?}, value is {value_ty:?}"
        ));
    }
    // the any-member lane's write answers like a dynamic write's
    Ok(Some(if per_instance {
        setter_param_ty
    } else {
        Type::Any
    }))
}

/// The value-param type of the setter a store of `field` through a
/// receiver declared `cls` runs, generic or not, as the store's check
/// reads it; nothing is recorded for the lowering.
pub(crate) fn setter_param(checker: &Checker, ast: &Ast, cls: &str, field: &str) -> Option<Type> {
    use crate::ast::accessor_lookup::{accessor_setter_in_chain, declared_setter_in_chain};
    let hit = accessor_setter_in_chain(ast, cls, field)
        .or_else(|| declared_setter_in_chain(ast, cls, field))?;
    setter_value_ty(checker, ast, &hit).map(|(t, _)| t)
}

/// The value-param type of the setter `hit` names. A generic
/// declarer's (blade 3) substitutes its TypeVars through the walk's
/// substitution so the check runs against the instantiated type, and
/// answers the type args the mono pass needs; `None` when either does
/// not resolve.
fn setter_value_ty(
    checker: &Checker,
    ast: &Ast,
    hit: &crate::ast::accessor_lookup::AccessorHit,
) -> Option<(Type, Option<Vec<Type>>)> {
    let Some(Type::Function(params, _ret)) = checker.globals.get(&hit.fn_name) else {
        return None;
    };
    let at = 1 + usize::from(ast.method_argv_fns.contains(&hit.fn_name));
    let ty = params.get(at)?.clone();
    let Some(tps) = checker.generic_type_params.get(&hit.fn_name) else {
        return Some((ty, None));
    };
    let mut submap: std::collections::HashMap<String, Type> = std::collections::HashMap::new();
    for (k, ann) in &hit.subst {
        let t = crate::check_type_ann::resolve_type_ann_full(
            ann,
            &checker.aliases,
            &[],
            &checker.generic_alias_decls,
        )?;
        submap.insert(k.clone(), t);
    }
    let type_args = tps
        .iter()
        .map(|tp| submap.get(tp).cloned())
        .collect::<Option<Vec<_>>>()?;
    Some((
        crate::check::substitute_typevars(&ty, &submap),
        Some(type_args),
    ))
}
