//! An object literal whose properties are spelled in another order
//! than the struct type it is checked against: `const a: A = { x: 1,
//! b: "q" }` with `type A = { b: string; x: number }` is legal, since
//! assignability is by property name and §10.1.11.1 enumerates `a` in
//! creation order. The struct arms of the assignability lattice
//! compare fields by position (class prefix subtyping depends on
//! that), so the literal's type is first put in the declared order.
//!
//! Only a literal written directly in a position whose lowering stores
//! it in the declared layout is reordered: a let / const initializer,
//! an assignment, a field store, a `return`, a call argument, a
//! `yield`, a `push` argument, and, under one of those, a nested
//! literal in a struct-typed field or an element of an array literal
//! whose element type is the struct. A nullable slot (`A | null`,
//! `A | undefined`, an optional field) stores the struct itself, so
//! its inner type is the one the literal is put in. The lowering reads
//! the storage order back off the literal's recorded type, which is
//! why every reordered literal records it.
//!
//! A value that first flows through an unannotated binding keeps its
//! own order and is still refused: its cell is laid out in that order,
//! and a read at the declared offsets would answer the wrong slot.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

/// `value_ty` in the field order of `to` when `value` is an object
/// literal (or an array literal of them, at any depth) spelling the
/// same plain property names in another order; otherwise `value_ty`
/// unchanged.
pub(crate) fn in_declared_order(
    checker: &mut Checker,
    ast: &Ast,
    to: &Type,
    value: ExprId,
    value_ty: Type,
) -> Type {
    let mut records = Vec::new();
    let Some(ty) = reorder(checker, ast, to, value, &value_ty, &mut records) else {
        return value_ty;
    };
    // the literal's recorded type is its storage order: the lowering
    // stores the cell in that layout whether or not its position names
    // the declared layout
    checker.expr_types.extend(records);
    ty
}

fn reorder(
    checker: &Checker,
    ast: &Ast,
    to: &Type,
    value: ExprId,
    value_ty: &Type,
    records: &mut Vec<(ExprId, Type)>,
) -> Option<Type> {
    let to = crate::check::resolve_class_ref(
        to,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    );
    // the null arm of a nullable slot has no layout; a cell in it is
    // stored as the inner struct
    if let Type::Nullable(inner) = &to {
        return reorder(checker, ast, inner, value, value_ty, records);
    }
    match ast.get_expr(value) {
        Expr::ObjectLit { fields } => {
            reorder_object(checker, ast, &to, value, fields, value_ty, records)
        }
        Expr::Array(elems) => reorder_array(checker, ast, &to, value, elems, records),
        _ => None,
    }
}

fn reorder_object(
    checker: &Checker,
    ast: &Ast,
    to: &Type,
    value: ExprId,
    entries: &[(crate::ast::PropKey, ExprId)],
    value_ty: &Type,
    records: &mut Vec<(ExprId, Type)>,
) -> Option<Type> {
    let (Type::Struct(to_fields), Type::Struct(from_fields)) = (to, value_ty) else {
        return None;
    };
    let plain = entries.iter().all(|(n, _)| {
        crate::check_type_of_object_lit::spread_omit_set(n).is_none()
            && crate::check_type_of_object_lit::accessor_slot(n).is_none()
            && !crate::check_type_of_object_lit::private_slot(n)
    });
    if !plain || to_fields.len() != from_fields.len() {
        return None;
    }
    let mut out = Vec::with_capacity(to_fields.len());
    let mut changed = false;
    for (k, (name, to_ty)) in to_fields.iter().enumerate() {
        let pos = from_fields.iter().position(|(n, _)| n == name)?;
        changed |= pos != k;
        let from_ty = &from_fields[pos].1;
        // the last spelling of a repeated key is the one that lands
        let inner = entries.iter().rev().find(|(n, _)| n == name).map(|e| e.1);
        let ty = match inner.and_then(|e| reorder(checker, ast, to_ty, e, from_ty, records)) {
            Some(t) => {
                changed = true;
                t
            }
            None => from_ty.clone(),
        };
        out.push((name.clone(), ty));
    }
    if !changed {
        return None;
    }
    let ty = Type::Struct(out);
    records.push((value, ty.clone()));
    Some(ty)
}

/// An array literal under an array type: every element literal is put
/// in the element type's order. The array takes the reordered element
/// type only when all its elements then agree on it, the condition
/// under which the literal's own typing would have unified them;
/// otherwise nothing is recorded and the literal is refused as before.
fn reorder_array(
    checker: &Checker,
    ast: &Ast,
    to: &Type,
    value: ExprId,
    elems: &[ExprId],
    records: &mut Vec<(ExprId, Type)>,
) -> Option<Type> {
    let Type::Array(to_elem) = to else {
        return None;
    };
    let mark = records.len();
    let mut changed = false;
    let mut agreed: Option<Type> = None;
    for &e in elems {
        let ty = match ast.get_expr(e) {
            Expr::Spread { expr } => match checker.expr_types.get(expr) {
                Some(Type::Array(t)) => (**t).clone(),
                _ => return undo(records, mark),
            },
            // an empty inner literal takes its type from its siblings
            Expr::Array(els) if els.is_empty() => continue,
            _ => {
                let Some(ety) = checker.expr_types.get(&e).cloned() else {
                    return undo(records, mark);
                };
                match reorder(checker, ast, to_elem, e, &ety, records) {
                    Some(t) => {
                        changed = true;
                        t
                    }
                    None => ety,
                }
            }
        };
        match &agreed {
            Some(a) if *a != ty => return undo(records, mark),
            Some(_) => {}
            None => agreed = Some(ty),
        }
    }
    if !changed {
        return None;
    }
    let ty = Type::Array(Box::new(agreed?));
    records.push((value, ty.clone()));
    Some(ty)
}

fn undo(records: &mut Vec<(ExprId, Type)>, mark: usize) -> Option<Type> {
    records.truncate(mark);
    None
}
