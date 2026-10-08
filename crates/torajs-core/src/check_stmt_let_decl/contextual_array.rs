//! Contextual typing of array-literal initializers, split out of the
//! let-declaration checker (it serves call arguments and assignments
//! too).

use super::any_layout_slot;
use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, DiagPush, Type};

/// Chunk 702 — TS contextual typing for array-literal inits: after the
/// annotation is verified assignable, the literal's recorded type IS
/// the annotation, all the way down through nested literals. Before
/// this, `const anyz: any[][] = [[2]]` left the inner literal typed
/// `Array<Number>` (pure inference), so lowering minted a
/// typed-behind-any block and a kind-change mutator
/// (`anyz[0].unshift("y")`) hit the catchable-TypeError protocol that
/// exists to protect typed-ALIAS any-views — a literal has no typed
/// alias, so bun's accept semantics apply. Overwriting is
/// equal-or-widening only: assignability was just verified, and for a
/// non-Any annotation the contextual type matches the inference.
/// Spread elements aren't literals — recursion simply skips them (the
/// spread lowering keeps its own element-type derivation).
///
/// `pub(crate)` since rotation 412: a literal Array ARGUMENT against
/// a declared `T[]` param is the same contextual-typing story
/// ([`crate::check_type_of_call::general`] calls this per admitted
/// arg) — without it a uniform-kind literal against an `any[]` param
/// minted the typed flavor and the callee's Arr<Any> readers
/// mis-decoded the slots.
pub(crate) fn apply_contextual_array_ann(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    ann: &Type,
) {
    // an `any` element stays what it is: a struct slot would read its
    // box at the declared offsets, so the literal keeps the dynamic
    // layout its inference chose (`any_layout_slot`)
    if any_layout_slot::struct_elem_array(ann)
        && any_layout_slot::has_dynamic_elem(eid, ast, &checker.expr_types)
    {
        return;
    }
    apply_contextual_array_ann_store(checker, ast, eid, ann);
}

/// [`apply_contextual_array_ann`] for a literal stored into a binding
/// that already exists: it takes the binding's type as it is, which
/// is `any[]` for a struct-array `let` such a store widened
/// (`crate::let_widen`).
pub(crate) fn apply_contextual_array_ann_store(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    ann: &Type,
) {
    let Type::Array(elem_ann) = ann else { return };
    let Expr::Array(elements) = ast.get_expr(eid) else {
        return;
    };
    if any_layout_slot::struct_elem_array(ann)
        && let Some(Type::Array(inferred)) = checker.expr_types.get(&eid).cloned()
    {
        refuse_unfit_elems(checker, ast, elements, &inferred, elem_ann);
    }
    checker.expr_types.insert(eid, ann.clone());
    // The lowering flavor gate keys off this side-set, NOT off
    // expr_types — infer-widened Array<Any> shapes (`["a",
    // undefined]`) share the type but belong to the typed lane.
    if matches!(**elem_ann, Type::Any) {
        checker.contextual_any_literals.insert(eid);
    }
    for &el in elements {
        apply_contextual_array_ann(checker, ast, el, elem_ann);
    }
}

/// A struct-array literal was admitted by its inferred element type:
/// its first element's, or `any` when the elements share none (an `any`
/// element fits any slot). It now takes `T[]` and every element is read
/// as a `T`, so each object element of another type that is not itself
/// a literal (those are laid out to the declared type where they stand)
/// has to fit `T` on its own — a class instance that only matched the
/// first element's data fields included. TS refuses the declaration
/// otherwise, and the spec gives it no meaning.
fn refuse_unfit_elems(
    checker: &mut Checker,
    ast: &Ast,
    elements: &[ExprId],
    inferred: &Type,
    elem_ann: &Type,
) {
    for (i, &el) in elements.iter().enumerate() {
        if matches!(
            ast.get_expr(el),
            Expr::Spread { .. } | Expr::ObjectLit { .. } | Expr::Array(_)
        ) {
            continue;
        }
        let Some(t) = checker.expr_types.get(&el).cloned() else {
            continue;
        };
        if !matches!(t, Type::Struct(_) | Type::ClassRef(_)) || t == *inferred {
            continue;
        }
        let tables = (
            &checker.class_structs,
            &checker.aliases,
            &checker.generic_alias_decls,
        );
        if crate::check_assignable::is_assignable_to_resolved(
            elem_ann, &t, tables.0, tables.1, tables.2,
        ) || crate::check_assignable::fn_slot_admits(elem_ann, &t, tables.0, tables.1, tables.2)
        {
            continue;
        }
        checker.errors.push_err(format!(
            "type mismatch on array element {i}: declared {elem_ann:?}, element has {t:?}"
        ));
    }
}
