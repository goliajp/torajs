//! `Expr::Array` typecheck pulled out of [`crate::check::Checker::type_of_inner`]'s
//! `Expr::Array` arm as chunk-88 of the type_of_inner decomp.
//!
//! Inference rules:
//!
//! 1. **Empty `[]`** (P0.10) — non-let-init position defaults to
//!    `Array<Any>` per TS spec. Mirrors the LetDecl empty-`[]` default.
//!    Pre-fix tora rejected `new Array().length` / `[].length` /
//!    fn-arg empty arrays with the explicit-annotation demand.
//! 2. **Per-element value type** — for non-spread: `T` directly; for
//!    spread (`...src`): from `src` source type:
//!    - `Array<T>` → `T`
//!    - `String` (S134) → `String` (str spread per code unit; ssa_lower
//!      wires str_split + materialize)
//!    - `Set` (S141) → `Any` (Array.from(set) shape)
//!    - other → spread-source error.
//!    Empty inner array literals `[]` yield `None` so the outer
//!    typecheck can defer typing to a non-empty sibling.
//! 3. **Anchor type** — first non-empty element's type.
//! 4. **Heterogeneous widening** (T-10.c v0.4.0) — when any later
//!    element isn't assignable to the anchor, widen to `Array<Any>`
//!    (matches bun's `[1, 'a', true]: any[]` shape). Strict per-slot
//!    typing preserved when ALL elements share a type.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};
use crate::check_assignable::is_assignable_to_resolved;

pub(crate) fn check(checker: &mut Checker, ast: &Ast, elements: &[ExprId]) -> Result<Type, String> {
    if elements.is_empty() {
        return Ok(Type::Array(Box::new(Type::Any)));
    }
    let ids: Vec<ExprId> = elements.to_vec();
    let mut first_ty: Option<Type> = None;
    for &eid in &ids {
        if let Some(t) = elem_value_ty(checker, ast, eid)? {
            first_ty = Some(t);
            break;
        }
    }
    let first_ty = match first_ty {
        Some(t) => t,
        // All-empty inner literals (`[[]]`, `[[], []]`) — extend the
        // P0.10 empty-`[]` default one level: each inner `[]` is
        // `Array<Any>`, so the outer infers `Array<Array<Any>>`.
        None => Type::Array(Box::new(Type::Any)),
    };
    let mut heterogeneous = false;
    for &eid in ids.iter() {
        // No early break — every element must be WALKED even after
        // the widening verdict is settled: `type_of` records the
        // element's type in `expr_types`, which the Any-slot pack
        // reads to keep `undefined` and `null` apart (both lower to
        // ConstPtrNull — RFC 20260721 刀 12 G15: `[1, null, u]`
        // packed the unwalked `u` as null).
        let ty = elem_value_ty(checker, ast, eid)?;
        if let Some(ty) = ty
            && !heterogeneous
            && !is_assignable_to_resolved(
                &first_ty,
                &ty,
                &checker.class_structs,
                &checker.aliases,
                &checker.generic_alias_decls,
            )
        {
            heterogeneous = true;
        }
    }
    if heterogeneous {
        Ok(Type::Array(Box::new(Type::Any)))
    } else {
        Ok(Type::Array(Box::new(first_ty)))
    }
}

fn elem_value_ty(checker: &mut Checker, ast: &Ast, eid: ExprId) -> Result<Option<Type>, String> {
    if let Expr::Spread { expr } = ast.get_expr(eid) {
        let src_ty = checker.type_of(ast, *expr)?;
        return match src_ty {
            Type::Array(inner) => Ok(Some(*inner)),
            Type::String => Ok(Some(Type::String)),
            Type::Set => Ok(Some(Type::Any)),
            // `[...map]` (entry pairs) / `[...m.keys()/.values()/
            // .entries()]` / `[...set.values()]` — the collection and
            // its iterator cells drive the same unified runtime
            // iteration protocol as the `any` arm below, so the
            // materialized product is `Array<Any>` (type-erased,
            // mirroring the Set arm). Lowering boxes the source and
            // routes it through `ssa_lower_arr_from_any::emit`.
            Type::Map | Type::MapIter | Type::ArrIter => Ok(Some(Type::Any)),
            // RFC 20260704 S5+ — `[...anyval]` iterates at runtime
            // through the unified protocol; elements are type-erased.
            Type::Any => Ok(Some(Type::Any)),
            other => Err(format!(
                "array spread source must be an array, got {other:?}"
            )),
        };
    }
    if matches!(ast.get_expr(eid), Expr::Array(els) if els.is_empty()) {
        return Ok(None);
    }
    Ok(Some(checker.type_of(ast, eid)?))
}
