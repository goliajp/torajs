//! `Object.assign` / `Object.values` static-method early-
//! route arms extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 211 — fifth sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! Covers the 2 Object namespace static methods that have
//! polymorphic dispatch + arg-shape validation the regular
//! static-method table can't express:
//!
//! - `Object.assign(target, ...sources)` per §20.1.2.1.
//!   Copies own enumerable properties from each source into
//!   target, left-to-right. Subset constraint: target and
//!   every source must be the SAME struct type (no field-
//!   superset / partial / mismatched-shape yet). S127-3
//!   extends prior single-source MVP to N sources. Returns
//!   target so chains like `let r = Object.assign(...)` type-
//!   check.
//! - `Object.values(obj)` polymorphic over receiver:
//!   - Array → Array<T> (bun shallow-copy, spec §20.1.2.20)
//!   - String → Array<String> (per-char Str array, spec
//!     §22.1.5.2 + §20.1.2.20 + ToObject on a primitive
//!     string → indexed-properties walk)
//!   - Any → Array<Any> (W-J Phase C2 struct identity known
//!     only at runtime → SSA-lower routes through
//!     struct_enum walker)
//!   - Struct → Array<T> for a homogeneous struct (T = shared
//!     field type); rejects empty + heterogeneous structs.
//!
//! Returns `Some(Ok(_))` on match, `Some(Err(_))` on arg
//! shape mismatch, `None` when callee isn't one of the 2
//! matched names on the `Object` namespace.

use crate::ast::PropKey;
use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

/// A class instance is a struct; these two arms just never said so.
///
/// `Object.keys` / `Object.entries` / `JSON.stringify` all read a
/// `new C()` fine because they resolve the class reference to the shape
/// behind it. `Object.values` and `Object.assign` matched `Type::Struct`
/// directly, so the same object was a struct to three surfaces and a
/// `ClassRef` to two — a loud reject rather than a wrong answer, but the
/// same disagreement.
fn unwrap_class(checker: &Checker, ty: Type) -> Type {
    crate::check::resolve_class_ref(
        &ty,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    )
}

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    // `Object.assign(target, ...sources)` per §20.1.2.1 —
    // copy own enumerable properties from each source into
    // target, left-to-right. Subset constraint: target and
    // every source must be the SAME struct type (no field-
    // superset / partial / mismatched-shape yet). Static-
    // resolved at lower time as N×(field-by-field copy).
    // Returns target so chains like `let r = Object.assign(...)`
    // type-check. S127-3 extends prior single-source MVP to
    // N sources (closes the most common Object.assign idiom
    // — `assign({}, a, b)` merge — without runtime shape work).
    if let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "assign"
        && let Expr::Ident(ns) = ast.get_expr(*ns_id)
        && ns == "Object"
    {
        if args.is_empty() {
            return Some(Err(
                "Object.assign requires at least a target arg".to_string()
            ));
        }
        let target_ty = match checker.type_of(ast, args[0]) {
            Ok(t) => unwrap_class(checker, t),
            Err(e) => return Some(Err(e)),
        };
        // An `any` target takes the runtime §20.1.2.1 walk (the
        // anyv_assign kernel) — sources just need to typecheck; any
        // shape rides (struct literals box, any sources pass through).
        //
        // An empty-struct target (`Object.assign({}, ...)` idiom) has
        // no slot for any source prop by definition, so the static
        // subset gate has nothing to check against — route it through
        // the same runtime walk. SSA-lower allocates a fresh dynobj as
        // the real target (spec: assign returns target; an empty struct
        // has no observable identity, so the swap is invisible) and
        // answers Type::Any.
        let empty_struct_target = matches!(&target_ty, Type::Struct(f) if f.is_empty());
        if matches!(target_ty, Type::Any) || empty_struct_target {
            for src_id in &args[1..] {
                if let Err(e) = checker.type_of(ast, *src_id) {
                    return Some(Err(e));
                }
            }
            return Some(Ok(Type::Any));
        }
        let Type::Struct(t_fields) = &target_ty else {
            return Some(Err(format!(
                "Object.assign target must be a struct, got {target_ty:?}"
            )));
        };
        let sinks = own_property_sink_types(t_fields);
        for (i, src_id) in args[1..].iter().enumerate() {
            let source_ty = match checker.type_of(ast, *src_id) {
                Ok(t) => unwrap_class(checker, t),
                Err(e) => return Some(Err(e)),
            };
            let Type::Struct(s_fields) = &source_ty else {
                return Some(Err(format!(
                    "Object.assign source[{i}] must be a struct, got {source_ty:?}"
                )));
            };
            for (key, read_ty) in own_property_types(s_fields) {
                let Some((_, accepts)) = sinks.iter().find(|(k, _)| *k == key) else {
                    return Some(Err(format!(
                        "Object.assign source[{i}] has property `{key}`, which the target struct has no slot for; target={target_ty:?}"
                    )));
                };
                // A get-only target property accepts nothing — the write
                // is a strict-mode TypeError (ES §10.1.9), which SSA-lower
                // emits. It is a RUNTIME throw, so it must typecheck.
                let Some(write_ty) = accepts else { continue };
                if *write_ty != read_ty {
                    return Some(Err(format!(
                        "Object.assign source[{i}] property `{key}` is {read_ty:?}, but the target takes {write_ty:?}"
                    )));
                }
            }
        }
        return Some(Ok(target_ty));
    }
    // `Object.values(obj)` — result Array<T> for a
    // homogeneous struct (T = shared field type), resolved
    // at lower time like Object.keys but packing values.
    if let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "values"
        && let Expr::Ident(ns) = ast.get_expr(*ns_id)
        && ns == "Object"
        && args.len() == 1
    {
        let raw_ty = match checker.type_of(ast, args[0]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        // An Error-family instance owns non-enumerable slots the
        // layout cannot express, so lowering routes it through the
        // runtime own-walk (`error_family_receiver`) — the same lane
        // an `any` receiver rides, typed the same way. This must run
        // before `unwrap_class`: the homogeneity test below would
        // otherwise reject `class V extends Error { code = 7 }`
        // (its layout mixes the inherited String slots with Number)
        // for a walk that never reads the layout at all.
        if let Type::ClassRef(n) = &raw_ty
            && crate::ast::class_reaches_error(ast, n)
        {
            return Some(Ok(Type::Array(Box::new(Type::Any))));
        }
        let arg_ty = unwrap_class(checker, raw_ty);
        // W-O — Array receiver: bun returns a fresh shallow
        // array of slot values (spec §20.1.2.20); SSA-lower
        // reuses the typed-struct Arr-field deep-clone arm.
        if let Type::Array(elem) = &arg_ty {
            return Some(Ok(Type::Array(elem.clone())));
        }
        // W-O-2 — String receiver: bun returns the per-char
        // Str array (spec §22.1.5.2 + §20.1.2.20 + ToObject
        // on a primitive string → indexed-properties walk).
        if matches!(arg_ty, Type::String) {
            return Some(Ok(Type::Array(Box::new(Type::String))));
        }
        // W-J Phase C2 — `any` receiver: struct identity
        // (per-field types) known only at runtime → Array<Any>
        // (SSA-lower routes through the struct_enum walker).
        if matches!(arg_ty, Type::Any) {
            return Some(Ok(Type::Array(Box::new(Type::Any))));
        }
        // Cluster #4 follow-up (rotation 235) — a Function receiver:
        // the lowering boxes the closure cell and the runtime
        // own-values walk answers the expando props (a plain fn
        // answers [] — the §20.2.4 virtual face is non-enumerable).
        if matches!(arg_ty, Type::Function(..)) {
            return Some(Ok(Type::Array(Box::new(Type::Any))));
        }
        let Type::Struct(fields) = &arg_ty else {
            return Some(Err(format!(
                "Object.values requires a struct arg, got {arg_ty:?}"
            )));
        };
        // RFC 20260714-objlit-accessor blade 6 — an accessor contributes
        // the value its GETTER answers (ES §20.1.2.23 reaches every own
        // key through [[Get]]), so the homogeneity test runs over the
        // PROPERTIES, not the layout slots. Pre-fix `{a: 1, get v(): number}`
        // was rejected outright: the `__getter_v` slot is a Function and
        // "earlier fields are Number" (bun answers `[1, 2]`).
        let props = own_property_types(fields);
        // Rotation 545 — §20.1.2.22 has no homogeneity precondition:
        // a heterogeneous (or empty) struct answers `Array<Any>` and
        // the lowering routes the same boxed runtime own-walk the
        // `as any` guard already rides. Only the all-same-type case
        // keeps the narrowed `Array<T>`.
        if props.is_empty() || props.iter().skip(1).any(|(_, t)| t != &props[0].1) {
            return Some(Ok(Type::Array(Box::new(Type::Any))));
        }
        return Some(Ok(Type::Array(Box::new(props[0].1.clone()))));
    }
    None
}

/// The own properties a struct's layout enumerates, in declaration
/// order, with the type each ACCEPTS on a [[Set]]-based write — the
/// mirror of [`own_property_types`], for `Object.assign`'s target side
/// (RFC 20260714-objlit-accessor blade 8).
///
/// * a data field accepts its own type;
/// * `__setter_v` accepts its setter's PARAM type under the key `v`
///   (the setter half wins over its getter — a write goes through it);
/// * a get-only accessor accepts nothing (`None`) — ES §10.1.9 makes
///   the write a strict-mode `TypeError`, which SSA-lower emits, so it
///   must typecheck rather than be rejected.
pub(crate) fn own_property_sink_types(fields: &[(PropKey, Type)]) -> Vec<(PropKey, Option<Type>)> {
    let mut out: Vec<(PropKey, Option<Type>)> = Vec::new();
    for (name, ty) in fields {
        let (key, accepts) = match crate::check_type_of_object_lit::accessor_slot(name) {
            Some(("__setter_", prop)) => {
                let param = match ty {
                    Type::Function(params, _) => params.first().cloned().unwrap_or(Type::Any),
                    _ => Type::Any,
                };
                (PropKey::from(prop), Some(param))
            }
            Some((_, prop)) => (PropKey::from(prop), None),
            None => (name.clone(), Some(ty.clone())),
        };
        match out.iter_mut().find(|(k, _)| *k == key) {
            // A getter half seen first is superseded by its setter.
            Some(slot) if slot.1.is_none() => slot.1 = accepts,
            Some(_) => {}
            None => out.push((key, accepts)),
        }
    }
    out
}

/// The own properties a struct's layout enumerates, in declaration
/// order, with the type each contributes to a [[Get]]-based read
/// (`Object.values` / `Object.entries` / `JSON.stringify` all reach an
/// accessor through its getter — RFC 20260714-objlit-accessor).
///
/// * a data field contributes itself;
/// * `__getter_v` contributes the getter's RETURN type under the key `v`;
/// * `__setter_v` alone contributes `undefined` (ES §10.1.8 — an
///   accessor with no [[Get]]), and pairs with its getter into ONE
///   property when both halves are present.
pub(crate) fn own_property_types(fields: &[(PropKey, Type)]) -> Vec<(PropKey, Type)> {
    let mut out: Vec<(PropKey, Type)> = Vec::new();
    for (name, ty) in fields {
        let (key, prop_ty) = match crate::check_type_of_object_lit::accessor_slot(name) {
            Some(("__getter_", prop)) => {
                let ret = match ty {
                    Type::Function(_, ret) => (**ret).clone(),
                    _ => Type::Any,
                };
                (PropKey::from(prop), ret)
            }
            Some((_, prop)) => (PropKey::from(prop), Type::Undefined),
            None => (name.clone(), ty.clone()),
        };
        match out.iter_mut().find(|(k, _)| *k == key) {
            // The getter half wins over a lone setter's `undefined` —
            // declaration order between the two halves is irrelevant.
            Some(slot) if slot.1 == Type::Undefined => slot.1 = prop_ty,
            Some(_) => {}
            None => out.push((key, prop_ty)),
        }
    }
    out
}
