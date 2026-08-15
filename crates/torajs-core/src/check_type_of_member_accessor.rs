//! Visibility enforcement + accessor-read probes for the member
//! checker — split out of `check_type_of_member.rs` (file-size line,
//! rotation 296). Three faces:
//!
//! - `enforce_visibility` — M-OO.5 private/protected gate off the
//!   receiver's nominal class.
//! - `class_name_of` — RFC 20260715-nominal-class-identity: the
//!   receiver's class taken from its type's NAME (a bare `Struct`
//!   belongs to no class, however closely its shape matches one).
//! - `try_accessor_read` — the objlit `__getter_<name>` layout slot
//!   (RFC 20260714-objlit-accessor blade 2: the accessor lives IN
//!   the type, which is what makes it un-stealable) and the class
//!   `accessor_getters` probe (P8.2: `c.value` answers the getter's
//!   RETURN type per ES §10.1.7 [[Get]]).

use crate::ast::{Ast, Expr, ExprId, Visibility};
use crate::check::{Checker, Type, resolve_class_ref};

/// The two accessor-read probes, in the order the member checker ran
/// them inline: the objlit `__getter_` layout slot on the RESOLVED
/// struct shape, then the class-accessor table keyed off the
/// UNRESOLVED type's class name. `None` = not an accessor read.
pub(crate) fn try_accessor_read(
    checker: &mut Checker,
    ast: &Ast,
    site: ExprId,
    obj_ty: &Type,
    resolved_obj_ty: &Type,
    name: &str,
) -> Option<Result<Type, String>> {
    if let Type::Struct(fields) = resolved_obj_ty {
        // The probe name builds ONCE per struct read — a
        // per-comparison `format!` in the scan closure was O(fields)
        // allocs per member read (the try_objlit_setter mirror,
        // rotation 268 profile on the 75KB unicode-ident class file).
        let getter_name = format!("__getter_{name}");
        if let Some((_, ty)) = fields.iter().find(|(fname, _)| *fname == getter_name) {
            let Type::Function(_, ret) = ty else {
                return Some(Err(format!("accessor `{name}` is not a getter closure")));
            };
            return Some(Ok(resolve_class_ref(
                ret,
                &checker.class_structs,
                &checker.aliases,
                &checker.generic_alias_decls,
            )));
        }
    }
    // Blade 2 (rotation 413) — the pair may live on an ANCESTOR
    // (§10.1.7 walks the prototype chain). The walk starts from the
    // receiver's full ClassRef key — a generic instantiation
    // (`"Box<number>"`) seeds the heritage-argument composition, and
    // a hit on a generic declarer substitutes the getter's TypeVars
    // and seeds the monomorphizer (blade 3/4), so the read keeps its
    // declared type AND a concrete callee.
    let cls_key = match obj_ty {
        Type::ClassRef(n)
            if ast
                .class_parents
                .contains_key(n.split('<').next().unwrap_or(n)) =>
        {
            n.clone()
        }
        _ => return None,
    };
    let hit = crate::ast::accessor_lookup::accessor_getter_in_chain(ast, &cls_key, name)?;
    let Some(Type::Function(_params, ret)) = checker.globals.get(&hit.fn_name).cloned() else {
        return None;
    };
    if let Some(tps) = checker.generic_type_params.get(&hit.fn_name).cloned() {
        // Generic declarer — resolve each composed argument spelling,
        // substitute the return type, and record the site so the mono
        // pass emits (and the lowering retargets to) the
        // specialization. Any unresolvable piece falls through to the
        // any-lane, today's behavior.
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
        let type_args: Vec<Type> = tps
            .iter()
            .map(|tp| submap.get(tp).cloned())
            .collect::<Option<Vec<_>>>()?;
        let ret2 = crate::check::substitute_typevars(&ret, &submap);
        checker
            .generic_call_sites
            .insert(site, (hit.fn_name.clone(), type_args));
        return Some(Ok(resolve_class_ref(
            &ret2,
            &checker.class_structs,
            &checker.aliases,
            &checker.generic_alias_decls,
        )));
    }
    Some(Ok(resolve_class_ref(
        &ret,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    )))
}

/// M-OO.5 — visibility enforcement. Find the binding's nominal class:
///
///   - `this` inside a class method body inherits the current class
///     context.
///   - An Ident bound by `let x: ClassName = ...` carries its
///     `declared_class` from the LetDecl arm.
///
/// Other shapes (chained Member, Call result, etc.) currently get no
/// nominal info; treat their visibility as Public until that path needs
/// tightening.
pub(crate) fn enforce_visibility(
    checker: &Checker,
    ast: &Ast,
    obj: &ExprId,
    name: &str,
) -> Result<(), String> {
    let obj_class: Option<String> = match ast.get_expr(*obj) {
        Expr::This => checker.current_class.clone(),
        Expr::Ident(n) => checker.lookup(n).and_then(|info| info.declared_class),
        _ => None,
    };
    let Some(cls) = obj_class.as_deref() else {
        return Ok(());
    };
    let Some(vis) = ast
        .member_visibility
        .get(&(cls.to_string(), name.to_string()))
        .copied()
    else {
        return Ok(());
    };
    let allowed = match vis {
        Visibility::Public => true,
        Visibility::Private => checker.current_class.as_deref() == Some(cls),
        Visibility::Protected => checker
            .current_class
            .as_deref()
            .map(|c| c == cls || checker.is_descendant_of(ast, c, cls))
            .unwrap_or(false),
    };
    if !allowed {
        return Err(format!(
            "M-OO.5: cannot access {vis:?} member `{cls}.{name}` from {}",
            checker
                .current_class
                .as_deref()
                .map(|c| format!("class `{c}`"))
                .unwrap_or_else(|| "outside any class".to_string())
        ));
    }
    Ok(())
}

/// RFC 20260715-nominal-class-identity — the class a receiver belongs
/// to, taken from its type's NAME. A bare `Struct` (object literal, or
/// a `type P = {...}` alias) belongs to no class, however closely its
/// shape matches one — that is the whole point.
pub(crate) fn class_name_of(obj_ty: &Type, ast: &Ast) -> Option<String> {
    match obj_ty {
        Type::ClassRef(n) if ast.class_parents.contains_key(n) => Some(n.clone()),
        _ => None,
    }
}
