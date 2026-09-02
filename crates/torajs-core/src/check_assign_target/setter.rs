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
    let Some((_, Type::Function(params, _))) = setter else {
        let getter_name = format!("__getter_{field}");
        if fields.iter().any(|(n, _)| *n == getter_name) {
            return Err(format!(
                "cannot assign to `{field}`: it is a getter-only accessor"
            ));
        }
        return Ok(None);
    };
    let Some(param_ty) = params.first().cloned() else {
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
    // (§10.1.9 walks the prototype chain). A generic declarer's hit
    // substitutes the setter's value-param TypeVars (blade 3) so the
    // assignability check runs against the instantiated type; the
    // LOWERING of that hit still rides the any-lane (a setter has no
    // recorded call site to retarget yet — RFC residual).
    let Some(hit) = crate::ast::accessor_lookup::accessor_setter_in_chain(ast, &cls, field) else {
        return Ok(None);
    };
    let setter_fn = hit.fn_name.clone();
    let Some(Type::Function(params, _ret)) = checker.globals.get(&setter_fn).cloned() else {
        return Ok(None);
    };
    if params.len() < 2 {
        return Ok(None);
    }
    let mut setter_param_ty = params[1].clone();
    if let Some(tps) = checker.generic_type_params.get(&setter_fn).cloned() {
        let mut submap: std::collections::HashMap<String, Type> = std::collections::HashMap::new();
        for (k, ann) in &hit.subst {
            let Some(t) = crate::check_type_ann::resolve_type_ann_full(
                ann,
                &checker.aliases,
                &[],
                &checker.generic_alias_decls,
            ) else {
                return Ok(None);
            };
            submap.insert(k.clone(), t);
        }
        let Some(type_args) = tps
            .iter()
            .map(|tp| submap.get(tp).cloned())
            .collect::<Option<Vec<_>>>()
        else {
            return Ok(None);
        };
        setter_param_ty = crate::check::substitute_typevars(&setter_param_ty, &submap);
        // Record the write site so the mono pass emits the setter
        // specialization and the lowering retargets its direct call.
        checker
            .generic_call_sites
            .insert(site, (setter_fn.clone(), type_args));
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
    Ok(Some(setter_param_ty))
}
