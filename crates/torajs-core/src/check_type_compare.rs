//! Type-shape comparison helpers pulled out of [`crate::check`] as
//! chunk-320 of the check.rs god-file decomp.
//!
//! Two pure functions, no `Checker` state:
//!
//! - `unify_ternary` — V3-18 wedge: ternary branch unification. Returns
//!   the join type if `t` and `e` can unify (equal types, `T | null`
//!   widen, mixed-Any wedge S129-1), else `None`.
//! - `struct_is_prefix_subtype` — M5.2: structural-prefix subtyping for
//!   class-method receiver arguments. `Dog` (fields: name, bark_count)
//!   is a valid receiver for an `Animal`-typed method (fields: name)
//!   because the `name` field sits at the same offset in both layouts.
//!
//! - `class_lca` — rotation 507: nearest common ancestor of two
//!   declared classes, the join every "two different class instances
//!   meet here" site needs (ternary branches, `??`, `||` / `&&`).
//!
//! Both grouped here as the "type-shape comparison" family. Re-exported
//! back into `crate::check` so callers
//! (`check_type_of_ternary::unify_ternary`,
//! `check_type_of_call_class_method_subtype::struct_is_prefix_subtype`)
//! keep their import paths.

use crate::ast::Ast;
use crate::check::Type;

pub(crate) fn unify_ternary(t: &Type, e: &Type) -> Option<Type> {
    if t == e {
        return Some(t.clone());
    }
    // S129-1 mixed-Any wedge — mirror of S128-5 LAnd/LOr / S127-4
    // indexOf 2-arg Any-escape. `b ? typed : (x: any)` widens the
    // result to Any; ssa-lower NaN-boxes the non-Any branch so the
    // shared __tern slot stays uniform.
    if matches!(t, Type::Any) || matches!(e, Type::Any) {
        return Some(Type::Any);
    }
    match (t, e) {
        // S2.27 (RFC 20260727-dstr-assignment 刀 5) — an Undefined
        // branch against any other type widens to Any, the S129-1
        // shape (ssa-lower boxes both branches into the shared
        // __tern slot; the Undefined side carries its ANY_UNDEF tag
        // through the expr-aware box). This is the destructuring
        // default's guard ternary over a MONOMORPHIC
        // `Array<Undefined>` source (`let [u = 13] = [undefined]`),
        // which used to reject with "branches differ". TS spells the
        // join `T | undefined`; Any is the same safe widening the
        // mixed-Any wedge already applies.
        (Type::Undefined, _) | (_, Type::Undefined) => Some(Type::Any),
        // rotation 233 (S2.27 field depth) — two structs with the
        // same field names in the same order and field-wise joinable
        // types (each field through this same join, so a
        // Number/Undefined or Number/Null field widens like the
        // top-level wedge) join to ANY, the S129-1 posture. The
        // iterator idiom `i <= 3 ? {value: i, done: false} : {value:
        // undefined, done: true}` is this shape; rotation 230's
        // knife 5 only caught the TOP-level Undefined.
        //
        // Any — NOT Struct(joined-fields): the branches materialize
        // by their OWN layouts, so a static join struct would read a
        // raw-I64 field where the other branch stored a sentinel
        // cell (probe: `d1.v` printed the pointer). The Any join
        // rides the ternary lowering's both-sides box and the
        // any-member read's runtime-layout normalization, which
        // answers every field combination correctly. A statically-
        // typed relayout join is the ceiling — L3b.
        (Type::Struct(tf), Type::Struct(ef)) => {
            if tf.len() != ef.len() {
                return None;
            }
            for ((tn, tt), (en, et)) in tf.iter().zip(ef.iter()) {
                if tn != en {
                    return None;
                }
                match (tt, et) {
                    (Type::Null, o) | (o, Type::Null) if !matches!(o, Type::Null) => {}
                    _ => {
                        unify_ternary(tt, et)?;
                    }
                }
            }
            Some(Type::Any)
        }
        // rotation 284 — an `Array(Any)` branch against any other
        // array joins to ANY, the S129-1 posture (`b ? [1, 2] : seq`
        // where `seq` grew from an empty literal). Both branches ride
        // the ternary lowering's both-sides box: the typed side's
        // block is boxed with its kind mark, so every consumer goes
        // through the kind-aware any lanes. A static Array(Any) join
        // would hand the typed block to an Arr<Any> reader WITHOUT
        // the mark — the same wrong-repr trap the S129-1 comment
        // records for scalars.
        (Type::Array(ta), Type::Array(ea))
            if matches!(**ta, Type::Any) || matches!(**ea, Type::Any) =>
        {
            Some(Type::Any)
        }
        // rotation 284 — an `Array(Any)` branch against a class
        // instance joins to ANY, same posture. The dstr default
        // guard mints this shape (`const [[,] = g()] = [[]]`: the
        // source element types Array(Any), the default a generator
        // ClassRef); both sides box and the downstream destructure
        // walks the any iterator protocol.
        (Type::Array(el), Type::ClassRef(_)) | (Type::ClassRef(_), Type::Array(el))
            if matches!(**el, Type::Any) =>
        {
            Some(Type::Any)
        }
        // rotation 362 — a register-repr branch against `null` joins
        // to ANY (S129-1 posture, mirror of the Undefined arm):
        // Nullable<Number>/Nullable<Boolean> has no SSA repr — the
        // F64 lane refuses ConstPtrNull loudly ("FPR materialization")
        // and the I64/Bool lanes would store the null ptr's zero bits
        // silently (probe: `c > 5 ? 7 : null` printed 0). A struct
        // branch joins to Any for the rotation-233 reason: its
        // Nullable join stored ConstPtrNull in an Obj-typed slot,
        // which every consumer read as a live layout (probe:
        // `c > 5 ? {a: 1} : null` printed [unknown-any-tag]). Ptr-repr
        // types (String / arrays / closures …) keep the Nullable join
        // below — their slot holds ConstPtrNull natively.
        (Type::Null, o) | (o, Type::Null)
            if matches!(o, Type::Number | Type::Boolean | Type::Struct(_)) =>
        {
            Some(Type::Any)
        }
        // rotation 400 (398-07) — two DIFFERENT concrete scalars join
        // to ANY, the S129-1 posture (`x === undefined ? "undef" : 1`;
        // TS spells the join `"undef" | 1`, a union tr approximates as
        // Any). Both branches ride the ternary lowering's both-sides
        // box, so every consumer reads a tagged AnyValue — the same
        // lane the Undefined arm above already exercises for each of
        // these types individually.
        (Type::String, Type::Number | Type::Boolean)
        | (Type::Number, Type::String | Type::Boolean)
        | (Type::Boolean, Type::String | Type::Number) => Some(Type::Any),
        (Type::Null, other) | (other, Type::Null) => Some(Type::Nullable(Box::new(other.clone()))),
        (Type::Nullable(inner), other) | (other, Type::Nullable(inner)) => {
            if inner.as_ref() == other {
                Some(Type::Nullable(inner.clone()))
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(crate) fn struct_is_prefix_subtype(arg: &Type, param: &Type) -> bool {
    match (arg, param) {
        (Type::Struct(arg_fields), Type::Struct(param_fields)) => {
            if param_fields.len() > arg_fields.len() {
                return false;
            }
            for (i, (pn, pt)) in param_fields.iter().enumerate() {
                let (an, at) = &arg_fields[i];
                if an != pn || at != pt {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

/// Nearest common ancestor of two declared classes along
/// `ast.class_parents` (a class is its own ancestor), or `None`
/// when the chains never meet. Hop-bounded like
/// `Checker::is_descendant_of`, so a mutual-extends cycle cannot
/// spin it.
///
/// The `None` answer is what separates "these meet at a type" from
/// "these only meet at `any`": callers join to the ancestor when it
/// exists and widen to Any otherwise (the S129-1 posture — both
/// sides box, consumers read the any lanes).
pub(crate) fn class_lca(ast: &Ast, a: &str, b: &str) -> Option<String> {
    let chain = |start: &str| -> Vec<String> {
        let mut out = vec![start.to_string()];
        let mut cur = start;
        let mut hops = ast.class_parents.len() + 1;
        while let Some(parent) = ast.class_parents.get(cur).and_then(|p| p.as_deref()) {
            out.push(parent.to_string());
            cur = parent;
            hops -= 1;
            if hops == 0 {
                break;
            }
        }
        out
    };
    let a_chain = chain(a);
    chain(b).into_iter().find(|c| a_chain.contains(c))
}

/// Rotation 507 — the join two DIFFERENT declared-class types take:
/// their nearest common ancestor, or Any when they share none. Used
/// wherever the language lets two class instances meet in one slot;
/// returns `None` when the pair is not two declared classes, leaving
/// the caller's own rules in charge.
pub(crate) fn class_join(ast: &Ast, a: &Type, b: &Type) -> Option<Type> {
    let (Type::ClassRef(an), Type::ClassRef(bn)) = (a, b) else {
        return None;
    };
    if an == bn
        || !ast.class_parents.contains_key(an.as_str())
        || !ast.class_parents.contains_key(bn.as_str())
    {
        return None;
    }
    Some(match class_lca(ast, an, bn) {
        Some(lca) => Type::ClassRef(lca),
        None => Type::Any,
    })
}
