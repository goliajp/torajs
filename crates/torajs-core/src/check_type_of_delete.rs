//! `delete <Member|Index>` (ES §13.5.1) — the type-only half of
//! the operator, moved out of [`crate::check_type_of_misc`] whole.
//!
//! It left because it is the one family in that file that is not a
//! "small self-contained arm": it carries two helpers of its own —
//! the receiver admission table and the error-message builder — and
//! between them they were a third of the file. The host keeps the
//! seven arms its module doc names; this answers one question,
//! **which receivers can say "no longer here"**, and the admission
//! table is where that question is decided.
//!
//! Pure position — the three items are byte-for-byte what they were.

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};

/// ES §13.5.1 — `delete <Member|Index>` → Boolean. The parser only
/// admits property-reference operands; here the receiver must be
/// one whose storage can say "no longer here" (below), and an Index
/// key must be one the property-key domain can name. Only the
/// receiver and key are typechecked, never the property itself:
/// deleting an absent key is legal and answers true.
/// Which receivers reach the OrdinaryDelete kernel.
///
/// Everything here turns on one question: can the storage say "no
/// longer here"? Deleting is not writing a blank — it has to remove an
/// own property, and a slot that can only hold a value of its element
/// type has no value that means absent.
///
/// - `any` — the dynobj / expando world the kernel was written for.
/// - An `any`-ELEMENT array — its slots already hold boxed values, so a
///   hole is something a slot can express. The delete leaves a real one
///   (length untouched, the index no longer own, the read answering
///   undefined, JSON and Object.keys agreeing with bun), and refcounted
///   elements release correctly.
/// - A `number[]` — its F64 lane spells a hole with the undefined
///   sentinel under a shadow mark (the width analysis seeds a delete
///   target's element slot F64, as it does an unproven read); a
///   `string[]` or an array of objects / arrays / functions — a NULL
///   slot under the same mark.
/// - A struct or class instance. A declared member's removal is kept
///   in the instance's sidecar, which every read that can meet it
///   consults: the any lane answers undefined, and a typed read of a
///   member that some delete in the program names takes the any lane
///   too (`named_prop_census::delete_hit`). The kernel removes it the
///   same way whether the receiver arrived typed or as `any`.
///
/// Out, and not for want of a lowering:
///
/// - A `boolean[]`: its slots have no spare value. Admitting it would
///   trade a compile-time refusal for a run-time throw. (A `number[]`
///   the width analysis could not move off an integer lane is refused
///   by the lowering for the same reason.)
///
/// The gate lifts when its slot can express absence.
fn receiver_admits_delete(obj_ty: &Type) -> bool {
    match obj_ty {
        Type::Any => true,
        Type::Array(elem) => {
            matches!(**elem, Type::Any | Type::Number | Type::String)
                || crate::ssa_lower_nullable_guard::spells_undef_with_generic_cell(elem)
        }
        // §13.5.1.2 ToObject over a string primitive — the "can the
        // storage say no-longer-here" question never arises: §10.4.3
        // makes the wrapper's own face (in-range indices, `length`)
        // non-configurable, so every delete is either the strict
        // refusal throw or an absent-key true. The kernel's Str arm
        // expresses both; nothing is ever removed.
        Type::String => true,
        // A namespace stand-in (`Type::Object` — Math / JSON /
        // Reflect / console …) is a real dynobj at runtime whose
        // entries carry attributes, so a slot CAN say no-longer-here.
        // Load-bearing since rotation 382 gave those objects real
        // `@@toStringTag` entries: the spec makes that one
        // configurable, so `delete Math[Symbol.toStringTag]` has to
        // both succeed and take the badge with it.
        Type::Object(_) => true,
        // 633-02 — a function object's own `length` and `name` are
        // configurable (§20.2.4), and at runtime it is a closure cell
        // with a props bag, so a slot CAN say no-longer-here: the any
        // lane already answered `delete f.length` exactly as bun does.
        // What kept this out was not the storage but the lowering — a
        // named fn read as a value is a bare address with no cell to
        // box; `ssa_lower_delete::lower_receiver` reifies it through
        // the canonical singleton the accessor face mints.
        Type::Function(_, _) => true,
        Type::Struct(_) | Type::ClassRef(_) => true,
        _ => false,
    }
}

fn delete_receiver_error(obj_ty: &Type) -> String {
    if let Type::Array(elem) = obj_ty {
        return format!(
            "`delete` on an array of {elem:?} is not supported: an unboxed \
             element slot has no value that means absent, so it cannot hold \
             a hole. Only an `any`-element array can"
        );
    }
    format!(
        "`delete` receiver must be an `any`-typed object or an `any`-element \
         array (got {obj_ty:?}); typed layouts have no removable properties"
    )
}

pub(crate) fn check_delete(
    checker: &mut Checker,
    ast: &Ast,
    operand: ExprId,
) -> Result<Type, String> {
    match ast.get_expr(operand) {
        crate::ast::Expr::Member { obj, name } => {
            // G2.5 — `delete globalThis.x` takes the builtin-name
            // gate; expando deletes ride the singleton's dynobj lane
            // like the member-store arm (check_type_of_assign).
            crate::check_type_of_assign::reject_globalthis_mutation(
                checker,
                ast,
                *obj,
                Some(name.as_str()),
            )?;
            let obj_ty = checker.type_of(ast, *obj)?;
            // A NAMED member on a typed array admits (rotation 410):
            // the hole argument gates only ELEMENT slots, and a name
            // is never an element — `length` is the permanent
            // non-configurable refusal (§10.4.2, the String-receiver
            // argument verbatim), an expando rides the props table
            // the kernel already deletes from, and an absent name
            // answers true. The Index arm below keeps the element
            // refusal: a numeric key names an unboxed slot.
            if receiver_admits_delete(&obj_ty) || matches!(&obj_ty, Type::Array(_)) {
                Ok(Type::Boolean)
            } else {
                Err(delete_receiver_error(&obj_ty))
            }
        }
        crate::ast::Expr::Index { obj, index } => {
            // G2.5 — the Index spelling of the globalThis delete gate:
            // string-literal keys resolve to the builtin-name check.
            crate::check_type_of_assign::reject_globalthis_mutation(
                checker,
                ast,
                *obj,
                crate::check_type_of_assign::literal_index_key(ast, *index),
            )?;
            let obj_ty = checker.type_of(ast, *obj)?;
            let idx_ty = checker.type_of(ast, *index)?;
            // The Index spelling of the typed-array NAMED admit
            // (rotation 410, Member-arm mirror): a string-literal key
            // that is not a canonical numeric string never names an
            // element slot, so the hole argument does not gate it —
            // `arr["length"]` is `arr.length` in brackets. A numeric
            // spelling (or any non-literal key, which could be one at
            // run time) keeps the element refusal.
            let named_literal_key = matches!(&obj_ty, Type::Array(_))
                && matches!(ast.get_expr(*index), crate::ast::Expr::String(s)
                    if !s.as_bytes().iter().all(|b| b.is_ascii_digit()) || s.is_empty());
            if !receiver_admits_delete(&obj_ty) && !named_literal_key {
                return Err(delete_receiver_error(&obj_ty));
            }
            // §6.1.7 — a Symbol is the other half of the property-key
            // domain, so `delete o[sym]` is as ordinary as the string
            // form (§13.5.1.2 → §10.1.10 OrdinaryDelete, which is
            // key-kind agnostic).
            // An `any` key is admitted on the same grounds: §7.1.19 is
            // defined on the value, so the kind is the run time's to
            // decide, and the lowering resolves it there. Rejecting it
            // here refused whole programs for `delete o[sym]` whenever
            // the symbol travelled in an `any`.
            // §7.1.19 step 3 is ToString for everything that is not a
            // Symbol, so a Number key names the very entry its string
            // spelling names — `o[1]` and `o["1"]` are two spellings of
            // one property. The lowering already coerces it: its
            // non-Symbol arm hands I64/F64 to the same `i64_to_str` /
            // `f64_to_str` kernels every other stringification asks, so
            // `-0` reaches "0" and `1e21` reaches "1e+21" as §7.1.17
            // requires. This gate had no missing implementation behind
            // it.
            //
            // BigInt and Boolean take the same road: §7.1.17 answers
            // "12" for `12n` and "true" for `true`, and the key coercer
            // is that ToString (rotation 689 split the console face,
            // which appended the `n` suffix, out of it — the reason
            // BigInt used to be refused here).
            if !matches!(
                idx_ty,
                Type::String
                    | Type::Symbol
                    | Type::Any
                    | Type::Number
                    | Type::BigInt
                    | Type::Boolean
            ) {
                return Err(format!(
                    "`delete` key must be a string, number, bigint, boolean or symbol (got {idx_ty:?})"
                ));
            }
            Ok(Type::Boolean)
        }
        _ => Err("`delete` target must be a property reference (obj.k / obj[k])".into()),
    }
}
