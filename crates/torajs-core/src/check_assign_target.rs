//! `Expr::Assign { target: Expr::Member | Expr::Index, value }`
//! typecheck sub-arms pulled out of
//! [`crate::check::Checker::type_of_inner`]'s `Expr::Assign` arm
//! as chunk-94 of the type_of_inner decomp.
//!
//! Two sub-paths bundled (matches the dispatch shape of the inner
//! match on the target's `Expr` variant):
//!
//! - **`obj.field = value` (Member-target)** — M1.4 / M-OO.5 / P3.2
//!   / T-27 / T-29 / P9.4 / P8.2 / V3-05 / V3-06 dispatch cascade:
//!   1. **M-OO.5 readonly enforcement** — `ast.readonly_fields`
//!      lookup; reject writes from outside the same class (TS
//!      restricts to constructor only; we approximate with same-
//!      class). Visibility (Private/Protected) was enforced
//!      already by the receiver `type_of(obj)` read-path traversal.
//!   2. **Type::Any** (P3.2) — dynobj_set; accept any value.
//!   3. **Type::Function** (T-27) — closure props_dynobj; accept
//!      any value.
//!   4. **Type::Array** (T-29) — arrprops side-table; accept any
//!      value.
//!   5. **Type::RegExp + "lastIndex"** (P9.4) — accept Number RHS.
//!   6. **Type::Struct** receiver:
//!      - **Accessor setter** (P8.2) — `accessor_setters[(cls, field)]`
//!        lookup; validate RHS against the setter's value-param
//!        type (`__this` first, then user-declared param).
//!      - **V3-06 empty array literal** — `this.kids = []` in ctor:
//!        bare empty literal gets element type from declared field
//!        array type.
//!      - **Direct struct field** (V3-05) — `is_assignable_to_resolved`
//!        on field_ty vs value_ty.
//!      Consume the value expr; return the assigned-to type.
//! - **`arr[i] = value` (Index-target)** — M1.4: obj must be
//!   `Array<T>`; index must be Number; value type must match elem.
//!   `Array<Any>[i] = <concrete>` allowed per TS spec (box at
//!   ssa-lower via `__torajs_arr_set_any`).
//! - **fallback** — `Err("invalid assignment target")`.
//!
//! Caller dispatch: `check_assign_target` matches on the inner
//! Expr variant and routes to the right helper.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::resolve_class_ref;
use crate::check::{Checker, Type};
use crate::check_assignable::is_assignable_to_resolved;

pub(crate) fn check_member(
    checker: &mut Checker,
    ast: &Ast,
    obj: ExprId,
    field: String,
    value: ExprId,
) -> Result<Type, String> {
    let obj_ty = checker.type_of(ast, obj)?;
    enforce_readonly(checker, ast, obj, &field)?;
    if matches!(obj_ty, Type::Any | Type::Function(..) | Type::Array(_)) {
        let _ = checker.type_of(ast, value)?;
        checker.consume(ast, value);
        return Ok(Type::Any);
    }
    if matches!(obj_ty, Type::RegExp) && field == "lastIndex" {
        let value_ty = checker.type_of(ast, value)?;
        if !matches!(value_ty, Type::Number) {
            return Err(format!(
                "type mismatch assigning to `RegExp.lastIndex`: expected number, got {value_ty:?}"
            ));
        }
        checker.consume(ast, value);
        return Ok(Type::Number);
    }
    let Type::Struct(fields) = &obj_ty else {
        return Err(format!(
            "field assignment target must be a struct, got {obj_ty:?}"
        ));
    };
    if let Some(t) = try_accessor_setter(checker, ast, &obj_ty, &field, value)? {
        return Ok(t);
    }
    let Some((_, field_ty)) = fields.iter().find(|(n, _)| n == &field) else {
        return Err(format!("no field `{field}` on type {obj_ty:?}"));
    };
    let field_ty = field_ty.clone();
    // V3-06 — `this.kids = []` in a class constructor: bare empty
    // array literal gets its element type from the field's declared
    // array type.
    if matches!(ast.get_expr(value), Expr::Array(els) if els.is_empty())
        && matches!(
            resolve_class_ref(&field_ty, &checker.aliases, &checker.generic_alias_decls),
            Type::Array(_)
        )
    {
        checker.consume(ast, value);
        return Ok(field_ty);
    }
    let value_ty = checker.type_of(ast, value)?;
    if !is_assignable_to_resolved(
        &field_ty,
        &value_ty,
        &checker.aliases,
        &checker.generic_alias_decls,
    ) {
        return Err(format!(
            "type mismatch assigning to `{field}`: field is {field_ty:?}, value is {value_ty:?}"
        ));
    }
    checker.consume(ast, value);
    Ok(field_ty)
}

fn enforce_readonly(checker: &Checker, ast: &Ast, obj: ExprId, field: &str) -> Result<(), String> {
    let obj_class: Option<String> = match ast.get_expr(obj) {
        Expr::This => checker.current_class.clone(),
        Expr::Ident(n) => checker.lookup(n).and_then(|info| info.declared_class),
        _ => None,
    };
    let Some(cls) = obj_class.as_deref() else {
        return Ok(());
    };
    if !ast
        .readonly_fields
        .contains(&(cls.to_string(), field.to_string()))
    {
        return Ok(());
    }
    if checker.current_class.as_deref() != Some(cls) {
        return Err(format!(
            "M-OO.5: cannot write readonly member `{cls}.{field}` from {}",
            checker
                .current_class
                .as_deref()
                .map(|c| format!("class `{c}`"))
                .unwrap_or_else(|| "outside any class".to_string())
        ));
    }
    Ok(())
}

fn try_accessor_setter(
    checker: &mut Checker,
    ast: &Ast,
    obj_ty: &Type,
    field: &str,
    value: ExprId,
) -> Result<Option<Type>, String> {
    let mut setter_class: Option<String> = None;
    for (n, ty) in checker.aliases.iter() {
        if ty == obj_ty && ast.class_parents.contains_key(n) {
            setter_class = Some(n.clone());
            break;
        }
    }
    let cls = match setter_class {
        Some(c) => c,
        None => return Ok(None),
    };
    let Some(setter_fn) = ast
        .accessor_setters
        .get(&(cls.clone(), field.to_string()))
        .cloned()
    else {
        return Ok(None);
    };
    let Some(Type::Function(params, _ret)) = checker.globals.get(&setter_fn).cloned() else {
        return Ok(None);
    };
    if params.len() < 2 {
        return Ok(None);
    }
    let setter_param_ty = params[1].clone();
    let value_ty = checker.type_of(ast, value)?;
    if !is_assignable_to_resolved(
        &setter_param_ty,
        &value_ty,
        &checker.aliases,
        &checker.generic_alias_decls,
    ) {
        return Err(format!(
            "type mismatch assigning to accessor `{cls}.{field}`: setter expects {setter_param_ty:?}, value is {value_ty:?}"
        ));
    }
    checker.consume(ast, value);
    Ok(Some(setter_param_ty))
}

pub(crate) fn check_index(
    checker: &mut Checker,
    ast: &Ast,
    obj: ExprId,
    index: ExprId,
    value: ExprId,
) -> Result<Type, String> {
    let obj_ty = checker.type_of(ast, obj)?;
    let idx_ty = checker.type_of(ast, index)?;
    if idx_ty != Type::Number {
        return Err(format!("index must be number, got {idx_ty:?}"));
    }
    // Any-dynamic-access RFC (20260704) S3-set — TS `any` admits
    // every index write; runtime dispatch (kind-aware Arr / silent
    // primitive no-op / explicit roadmap TypeError) happens in
    // `__torajs_any_index_set`. The value still typechecks for its
    // own side effects / diagnostics.
    if matches!(obj_ty, Type::Any) {
        let _ = checker.type_of(ast, value)?;
        return Ok(Type::Any);
    }
    let Type::Array(elem) = &obj_ty else {
        return Err(format!(
            "index assignment target must be an array, got {obj_ty:?}"
        ));
    };
    let elem_ty = (**elem).clone();
    let value_ty = checker.type_of(ast, value)?;
    if !is_assignable_to_resolved(
        &elem_ty,
        &value_ty,
        &checker.aliases,
        &checker.generic_alias_decls,
    ) {
        return Err(format!(
            "type mismatch on element assignment: array of {elem_ty:?}, value is {value_ty:?}"
        ));
    }
    checker.consume(ast, value);
    Ok(elem_ty)
}
