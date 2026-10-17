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
use crate::check::{Checker, DiagPush, Type};

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
    eid: ExprId,
    callee: &ExprId,
    args: &[ExprId],
) -> Option<Result<Type, String>> {
    // `Object.assign` is its own surface with its own subset rules;
    // it lives next door so this one stays about `Object.values`.
    if let Some(r) = try_match_assign(checker, ast, eid, callee, args) {
        return Some(r);
    }
    // A missing argument is `undefined`, and each of these begins
    // with ToObject on it (§20.1.2.{5,18,23} step 1)
    if args.is_empty()
        && let Expr::Member { obj, name } = ast.get_expr(*callee)
        && matches!(name.as_str(), "keys" | "values" | "entries")
        && matches!(ast.get_expr(*obj), Expr::Ident(ns) if ns == "Object")
    {
        return Some(nullish_to_object(checker, ast, eid, &[]));
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
        // 629-03 — a runtime-computed instance field is an expando the
        // layout does not enumerate, so the typed unfold would drop
        // its value (`class C { foo = 1; [x] = 42 }` answered `[1]`);
        // the runtime own-walk lists it at its declaration position.
        if let Type::ClassRef(n) = &raw_ty
            && crate::ast::class_has_computed_instance_fields(ast, n)
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
        // Three shapes, one answer: ask the runtime. A Function gives
        // its expando props (its §20.2.4 face is non-enumerable); a
        // bag-only shape gives its bag; a non-string primitive gives
        // its wrapper's own enumerable face, which §20.1.2.23 step 1's
        // ToObject makes empty. String took its own arm above because
        // its wrapper DOES carry index keys; nullish leaves via band B
        // below. All three box at the lowering and ride one walk.
        // (Bag list mirrors SSA's `shape_has_expando_bag` — separate
        // Type enums, so the two point at each other instead.)
        if matches!(
            arg_ty,
            Type::Function(..) | Type::Number | Type::Boolean | Type::BigInt | Type::Symbol
        ) || checker_shape_has_expando_bag(&arg_ty)
        {
            return Some(Ok(Type::Array(Box::new(Type::Any))));
        }
        // Band B, same step in §20.1.2.23 — see the Object.assign
        // arm above.
        if matches!(arg_ty, Type::Null | Type::Undefined) {
            return Some(nullish_to_object(checker, ast, eid, &[]));
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

/// `Object.assign(target, ...sources)` per §20.1.2.1 — split out of
/// [`try_match`] verbatim so each function is about one surface. The
/// body below is unchanged; `None` means the callee is not this
/// surface and the caller carries on to `Object.values`.
fn try_match_assign(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: &ExprId,
    args: &[ExprId],
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
        // ToObject(target) wraps a primitive too, and the wrapper is what
        // the call answers (`typeof Object.assign(1, {})` is "object") --
        // same runtime walk, reached the same way. BigInt stays out: the
        // runtime has no BigInt wrapper yet, so `Object(10n)` is identity
        // (a KNOWN GAP filed in to_object, not something to bodge here).
        let primitive_target = matches!(
            target_ty,
            Type::Number | Type::Boolean | Type::String | Type::Symbol
        );
        if matches!(target_ty, Type::Any) || empty_struct_target || primitive_target {
            for src_id in &args[1..] {
                if let Err(e) = checker.type_of(ast, *src_id) {
                    return Some(Err(e));
                }
            }
            return Some(Ok(Type::Any));
        }
        // Band B — §20.1.2.1 step 1 is ToObject(target), and
        // ToObject throws on null and undefined. The type is known
        // right here, so the throw is decided right here rather than
        // re-derived at run time; the program still compiles and
        // still throws. See `ssa_lower_spec_throw` and ch1 §4.
        if matches!(target_ty, Type::Null | Type::Undefined) {
            return Some(nullish_to_object(checker, ast, eid, &args[1..]));
        }
        // A RegExp target takes the runtime walk over a view of the
        // cell, whose own-property bag holds what no layout names; the
        // call answers the target itself, still a RegExp.
        if matches!(target_ty, Type::RegExp) {
            for src_id in &args[1..] {
                if let Err(e) = checker.type_of(ast, *src_id) {
                    return Some(Err(e));
                }
            }
            return Some(Ok(Type::RegExp));
        }
        let Type::Struct(t_fields) = &target_ty else {
            return Some(Err(format!(
                "Object.assign target must be a struct, got {target_ty:?}"
            )));
        };
        let sinks = own_property_sink_types((&*checker, ast), t_fields);
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
                        "Object.assign source[{i}] has property `{}`, which the target struct has no slot for; target={target_ty:?}",
                        key.lossy()
                    )));
                };
                // A get-only target property accepts nothing — the write
                // is a strict-mode TypeError (ES §10.1.9), which SSA-lower
                // emits. It is a RUNTIME throw, so it must typecheck.
                let Some(write_ty) = accepts else { continue };
                if *write_ty != read_ty {
                    return Some(Err(format!(
                        "Object.assign source[{i}] property `{}` is {read_ty:?}, but the target takes {write_ty:?}",
                        key.lossy()
                    )));
                }
            }
        }
        return Some(Ok(target_ty));
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
///   Band B for a nullish argument where the spec's first step is
///   ToObject — §20.1.2.1 (`Object.assign`) and §20.1.2.23
///   (`Object.values`) both begin there, and §7.1.18 answers null and
///   undefined with a TypeError.
///
/// The remaining arguments are still TYPED here even though the call
/// throws: §13.3.6.1 evaluates them before the call, so their side
/// effects are observable, and the lowering that emits the throw
/// reads `expr_types` to give each one its shape.
///
/// The message is the one `__torajs_anyv_assign` raises for the same
/// input, so what moved is when the decision is made, not what the
/// program prints.
fn nullish_to_object(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    rest: &[ExprId],
) -> Result<Type, String> {
    const MSG: &str = "Cannot convert undefined or null to object";
    for &a in rest {
        checker.type_of(ast, a)?;
    }
    checker.spec_throws.insert(eid, MSG.to_string());
    checker.errors.push_warn(MSG.to_string());
    Ok(Type::Any)
}

pub(crate) fn own_property_sink_types(
    cx: (&Checker, &Ast),
    fields: &[(PropKey, Type)],
) -> Vec<(PropKey, Option<Type>)> {
    let mut out: Vec<(PropKey, Option<Type>)> = Vec::new();
    for (name, ty) in fields {
        let (key, accepts) = match crate::check_type_of_object_lit::accessor_slot(name) {
            Some(("__setter_", prop)) => {
                let p = crate::check_type_of_object_lit::setter_value_ty(cx, fields, name, ty);
                (PropKey::from(prop), Some(p.unwrap_or(Type::Any)))
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

/// Whether this receiver keeps an ordinary own-property bag at runtime.
///
/// The checker-side twin of `ssa_lower_index::shape_has_expando_bag`.
/// The two cannot be one function — the checker and the SSA layer carry
/// separate `Type` enums — so they are two lists that must move
/// together, and both docs say so. The authority under both is
/// torajs-anyvalue's `member_get_layout::expando_props_off`.
///
/// Rotation 611 found seven hand-written copies of this question and
/// collapsed each into a call; these two are the irreducible remainder,
/// kept honest by pointing at each other.
fn checker_shape_has_expando_bag(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Map(..)
            | Type::Set
            | Type::RegExp
            | Type::Date
            | Type::Promise(..)
            | Type::MapIter(..)
            | Type::ArrIter
            | Type::WeakMap
            | Type::WeakSet
            | Type::WeakRef
    )
}
