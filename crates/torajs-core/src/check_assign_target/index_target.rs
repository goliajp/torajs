//! The INDEX-target half of `Expr::Assign` — `obj[k] = value` and
//! the key domain it admits.
//!
//! Split from the parent at the line its own module doc has drawn
//! since it was written ("Two sub-paths bundled"): everything here
//! answers "which keys may name a slot on this receiver, and what
//! does writing one mean", while the parent answers the same for a
//! NAME. The literal-key lane crosses back — `g[0] = v` is
//! `g."0" = v` per §7.1.19 — through the parent's `check_member`.

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};
use crate::check_assignable::is_assignable_to_resolved;

use super::check_member;

pub(crate) fn check_index(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    obj: ExprId,
    index: ExprId,
    value: ExprId,
) -> Result<Type, String> {
    // Cluster #6 (rotation 441) — the read side resolves a nominal
    // class instance to its field struct before dispatching
    // (`check_type_of_index` since RFC 20260715); the write side
    // never did, so every keyed WRITE on a `new C()` receiver fell
    // past all the struct arms into the array reject — the
    // cpn-class-expr-accessors family's `c[1 + 1] = 2`.
    let raw_obj_ty = checker.type_of(ast, obj)?;
    let obj_ty = crate::check::resolve_class_ref(
        &raw_obj_ty,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    );
    // Chunk 745 — struct receiver + compile-time literal index:
    // `g[0] = v` ≡ `g."0" = v` per ES ToPropertyKey (§7.1.19);
    // delegate to the member-assignment checker (field lookup /
    // readonly / setter / assignability). Mirrors the read-side
    // lane in `check_type_of_index`.
    if matches!(obj_ty, Type::Struct(_))
        && let Some(key) = crate::ast::literal_prop_key(ast, index)
        && let Some(name) = key.as_str()
    {
        // Bracket-spelled by construction: every path into this
        // module came from an `Expr::Index` target.
        return check_member(checker, ast, eid, obj, name.to_string(), value, true);
    }
    let idx_ty = checker.type_of(ast, index)?;
    reject_bad_key(&obj_ty, &idx_ty)?;
    if matches!(obj_ty, Type::Array(_)) && crate::check_type_of_index::array_key_domain(&idx_ty) {
        let _ = checker.type_of(ast, value)?;
        return Ok(Type::Any);
    }
    // Any-dynamic-access RFC (20260704) S3-set — TS `any` admits
    // every index write; runtime dispatch (kind-aware Arr / silent
    // primitive no-op / explicit roadmap TypeError) happens in
    // `__torajs_any_index_set`. The value still typechecks for its
    // own side effects / diagnostics.
    if matches!(obj_ty, Type::Any) {
        let key = crate::ast::literal_prop_key(ast, index);
        checker.check_dict_write(ast, obj, key.as_ref().and_then(|k| k.as_str()), value)?;
        return Ok(Type::Any);
    }
    // Rotation 346 — the struct + undefined-key pair the number
    // gate above admitted: the lowering boxes the struct receiver
    // and rides the keyed set kernel. String / Symbol / Any keys
    // ride the same box (cluster #3, rotation 442 — see the gate
    // note above); field types are heterogeneous so the static
    // answer is Any, the read side's mirror.
    if matches!(obj_ty, Type::Struct(_)) && crate::check_type_of_index::coerced_key_domain(&idx_ty)
    {
        let _ = checker.type_of(ast, value)?;
        return Ok(Type::Any);
    }
    // Cluster #6 (rotation 441) — a STRUCT receiver under a DYNAMIC
    // number key: §7.1.19 spells it in decimal at runtime, and the
    // keyed set kernel dispatches layout field / accessor / expando
    // exactly as the literal lane's member store would (the read
    // side has admitted this pair since chunk 753 — `Struct(_) =>
    // Ok(Any)` — so the write half was the asymmetry). The lowering
    // boxes the receiver and rides the keyed kernel.
    if matches!(obj_ty, Type::Struct(_)) && idx_ty == Type::Number {
        let _ = checker.type_of(ast, value)?;
        return Ok(Type::Any);
    }
    // A FUNCTION receiver — `h[0] = 11` on a fn value is the §7.1.19
    // stringified expando write member_set's closure arm owns (the
    // 405-01 residue closed the runtime in rotation 408; this admits
    // the TYPED spelling the any lane already reaches). The lowering
    // boxes the receiver and rides the any index-set kernel.
    if matches!(obj_ty, Type::Function(..))
        && (idx_ty == Type::Number || crate::check_type_of_index::coerced_key_domain(&idx_ty))
    {
        let _ = checker.type_of(ast, value)?;
        return Ok(Type::Any);
    }
    let Type::Array(elem) = &obj_ty else {
        // The singleton-backed namespaces again (member arm above)
        // — `Math[0] = 1` is a keyed expando write on the dynobj —
        // and the Map / Set / RegExp receivers whose reads have
        // answered `Any` since r289. The lowering boxes them and
        // rides the any set kernel; field types are heterogeneous so
        // the static answer is Any, the read side's mirror.
        if matches!(
            obj_ty,
            Type::Object(_) | Type::Map(..) | Type::Set | Type::RegExp
        ) {
            let _ = checker.type_of(ast, value)?;
            return Ok(Type::Any);
        }
        return Err(format!(
            "index assignment target must be an array, got {obj_ty:?}"
        ));
    };
    let elem_ty = (**elem).clone();
    let value_ty = checker.type_of(ast, value)?;
    if !is_assignable_to_resolved(
        &elem_ty,
        &value_ty,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    ) {
        return Err(format!(
            "type mismatch on element assignment: array of {elem_ty:?}, value is {value_ty:?}"
        ));
    }
    crate::check::face_slots::record_elem_store(checker, &elem_ty, &value_ty);
    // Chunk 567 — element assignment shares the rhs (mirror of the
    // member-store contract above); no consume.
    Ok(elem_ty)
}

/// §7.1.19 — whether `idx_ty` is a legal key for a WRITE to
/// `obj_ty`, or the loud reject. The write half of
/// [`crate::check_type_of_index`]'s gate of the same name, and it
/// has to be read against it: a receiver that ANSWERS a property
/// key on the read side has to be able to STORE one here, or the
/// two halves disagree about what a property is.
///
/// Was three `!(receiver && domain)` clauses inside one `&&` chain
/// with the receiver arms below spelling their domains a second
/// time — and the two spellings had already drifted apart: the
/// FUNCTION arm below listed `Any | String | Undefined` keys that
/// this gate rejected before it could ever see them, so three
/// quarters of that arm was dead code (measured, rotation 609).
/// A match says each receiver's domain once, where the arm below
/// can be read against it.
fn reject_bad_key(obj_ty: &Type, idx_ty: &Type) -> Result<(), String> {
    let admitted = match obj_ty {
        // L3b #13 / cluster #1 blade 4 — an `any` receiver admits
        // the whole property-key domain (§7.1.19 ToPropertyKey), the
        // coerced keys included: the keyed set kernel runs the
        // coercion at runtime off the key's tag.
        Type::Any => crate::check_type_of_index::coerced_key_domain(idx_ty),
        // Rotation 346 / cluster #3 (442) / cluster #4 (438) — a
        // STRUCT receiver takes the same domain: the keyed set
        // kernel's member_set struct arm dispatches layout field /
        // inherited accessor / expando off the spelled key, so a
        // dynamic string key no longer risks shadow-splitting a
        // layout slot — the cpn-class accessor family's
        // `c[String(1 + 1)] = 2`.
        Type::Struct(_) => crate::check_type_of_index::coerced_key_domain(idx_ty),
        // An ARRAY receiver takes the whole non-number domain, and
        // the read side's `array_key_domain` carries the reason:
        // §7.1.19 spells the key first and only then asks whether
        // the spelling is a canonical array index, so `x["0"] = 0`
        // reaches the ELEMENT store with its length update while
        // `x[true] = 1` stores under "true" and leaves `x[1]`
        // undefined — S15.4_A1.1_T4 and property-cast-boolean-
        // primitive, the two halves of one rule.
        Type::Array(_) => crate::check_type_of_index::array_key_domain(idx_ty),
        // A FUNCTION receiver takes the coerced domain — the read
        // side's arm of the same name carries the reason (a function
        // is an ordinary object with no element lane, so nothing has
        // to be told apart from a spelling). The store the arm below
        // reaches is the one the `any` lane has always run:
        // `fn[Symbol.isConcatSpreadable] = true`, and the four
        // TypedArray / RegExp-species cases that write a well-known
        // symbol onto a plain function.
        Type::Function(..) => crate::check_type_of_index::coerced_key_domain(idx_ty),
        // A MAP / SET / REGEXP receiver, and the singleton-backed
        // namespaces, take the whole non-number domain — the same
        // arms the read gate carries, and for the same reason: a
        // property store on one of them has no element lane to
        // collide with, so §7.1.19's spelling of the key is the
        // only thing that has to happen and the keyed set kernel
        // does it off the key's runtime tag.
        // The write side simply never had these arms, so a receiver
        // whose READ answers a property could not STORE one, and the
        // two halves disagreed about what a property is.
        Type::Map(..) | Type::Set | Type::RegExp | Type::Object(_) => {
            crate::check_type_of_index::coerced_key_domain(idx_ty)
        }
        _ => false,
    };
    if *idx_ty != Type::Number && !admitted {
        return Err(format!("index must be number, got {idx_ty:?}"));
    }
    Ok(())
}
