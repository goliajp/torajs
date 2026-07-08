//! Small `Expr` arms (TypeOf / InstanceOf / Nullish / OptChain /
//! PostIncr / Sequence / As) pulled out of
//! [`crate::check::Checker::type_of_inner`] as chunk-95 of the
//! type_of_inner decomp. 7 arms bundled because each is small and
//! self-contained (no SSA emission, pure type-only logic).
//!
//! - **`typeof <expr>`** (V3-18 m1.h.3 / m1.h.20) — always
//!   `Type::String`. Short-circuit for unresolved Idents and
//!   namespace-member dotted forms (`typeof globalThis` /
//!   `typeof Math.PI`) so check.rs doesn't bail on the global
//!   reference lookup.
//! - **`x instanceof C`** — operand typed; returns `Boolean`. Class
//!   resolution + static fold handled at ssa_lower.
//! - **`lhs ?? rhs`** — `Type::Any` lhs → rhs type; `Type::Nullable<T>`
//!   lhs + rhs of T → T; lhs + rhs of `Nullable<T>` → `Nullable<T>`;
//!   `Null`/`Undefined` lhs → rhs type; non-nullable lhs → lhs type
//!   (rhs is dead-branch but still typechecked).
//! - **`obj?.field`** (P3.5) — `Type::Any` per spec §13.3.9: hit
//!   yields boxed field value; miss yields ANY_UNDEF. `Type::Null`/
//!   `Undefined` obj → `Any`; plain (non-nullable) obj → concrete
//!   `member_type` result (semantically same as `.`).
//! - **`x++` / `x--`** — target must be `Number`; result `Number`.
//! - **`(a, b)`** comma — typecheck left for side effects, return
//!   right's type.
//! - **`expr as T`** — typecheck inner for side effects; non-null
//!   assertion `<expr>!` (encoded as `As { ty_ann: "__nonnull__" }`)
//!   narrows `Nullable<T>` → `T`; otherwise resolve the
//!   annotation.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type, is_known_builtin_global};
use crate::check_type_ann::resolve_type_ann_full;

pub(crate) fn check_typeof(checker: &mut Checker, ast: &Ast, expr: ExprId) -> Result<Type, String> {
    // V3-18 m1.h.3 / m1.h.20 — short-circuit known-builtin Ident
    // and Member-on-known-namespace; the spec result is the literal
    // string regardless of resolver state.
    if let Expr::Ident(name) = ast.get_expr(expr)
        && (checker.lookup(name).is_none() || is_known_builtin_global(name))
    {
        return Ok(Type::String);
    }
    if let Expr::Member { obj, .. } = ast.get_expr(expr)
        && let Expr::Ident(ns) = ast.get_expr(*obj)
        && is_known_builtin_global(ns)
    {
        return Ok(Type::String);
    }
    let _ = checker.type_of(ast, expr)?;
    Ok(Type::String)
}

pub(crate) fn check_instanceof(
    checker: &mut Checker,
    ast: &Ast,
    expr: ExprId,
) -> Result<Type, String> {
    let _ = checker.type_of(ast, expr)?;
    Ok(Type::Boolean)
}

pub(crate) fn check_nullish(
    checker: &mut Checker,
    ast: &Ast,
    lhs: ExprId,
    rhs: ExprId,
) -> Result<Type, String> {
    let lhs_ty = checker.type_of(ast, lhs)?;
    let rhs_ty = checker.type_of(ast, rhs)?;
    if matches!(lhs_ty, Type::Any) {
        return Ok(rhs_ty);
    }
    let lhs_inner = match &lhs_ty {
        Type::Nullable(inner) => Some((**inner).clone()),
        Type::Null => None,
        Type::Undefined => None,
        other => {
            // ES §13.4.2 — non-nullable typed lhs is a static no-op;
            // result is lhs's type (rhs dead-branch but still typed).
            let _ = rhs_ty;
            return Ok(other.clone());
        }
    };
    // If lhs was Null/Undefined literal, the answer is just rhs's type.
    let Some(inner) = lhs_inner else {
        return Ok(rhs_ty);
    };
    if rhs_ty == inner {
        return Ok(inner);
    }
    if let Type::Nullable(rhs_inner) = &rhs_ty
        && **rhs_inner == inner
    {
        return Ok(rhs_ty);
    }
    Err(format!(
        "`??` rhs type {rhs_ty:?} does not match lhs inner {inner:?}"
    ))
}

pub(crate) fn check_opt_chain(
    checker: &mut Checker,
    ast: &Ast,
    obj: ExprId,
    name: &str,
) -> Result<Type, String> {
    let obj_ty = checker.type_of(ast, obj)?;
    match &obj_ty {
        Type::Nullable(_) => {
            let inner_obj_ty = match &obj_ty {
                Type::Nullable(inner) => (**inner).clone(),
                _ => obj_ty.clone(),
            };
            let _ = checker.member_type(&inner_obj_ty, name)?;
            Ok(Type::Any)
        }
        Type::Null | Type::Undefined | Type::Any => Ok(Type::Any),
        _ => {
            // Plain (non-nullable) obj: `?.` ≡ `.`. Return concrete
            // member type since the optional path is dead.
            checker.member_type(&obj_ty, name)
        }
    }
}

/// Chunk 703 — `obj?.[index]` (ES2020 optional element access).
/// Same nullish contract as [`check_opt_chain`]: Nullable / Null /
/// Undefined / Any obj answers Any (the short-circuit path makes the
/// static element type unknowable); a plain obj is `?.` ≡ `[]` and
/// delegates to the Index checker. The index expression typechecks
/// in every branch — it may not EVALUATE on the short-circuit path
/// (lowering guards that), but it must still be well-typed.
pub(crate) fn check_opt_index(
    checker: &mut Checker,
    ast: &Ast,
    obj: ExprId,
    index: ExprId,
) -> Result<Type, String> {
    let obj_ty = checker.type_of(ast, obj)?;
    match &obj_ty {
        Type::Nullable(_) | Type::Null | Type::Undefined | Type::Any => {
            let _ = checker.type_of(ast, index)?;
            Ok(Type::Any)
        }
        _ => crate::check_type_of_index::check(checker, ast, obj, index),
    }
}

/// Chunk 705 — `callee?.(args…)` (ES2020 optional call). Same
/// nullish contract as [`check_opt_chain`]: a Nullable / Null /
/// Undefined / Any callee answers Any (nullish short-circuits to
/// undefined; a non-nullish value invokes through the runtime
/// any-call dispatch). A plain callee is statically non-nullish, so
/// `?.()` ≡ `()` and delegates to the Call checker (the lowering
/// mirrors the delegation). Args typecheck in every branch — they
/// may not EVALUATE on the short-circuit path.
pub(crate) fn check_opt_call(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: ExprId,
    args: &Vec<ExprId>,
) -> Result<Type, String> {
    let callee_ty = checker.type_of(ast, callee)?;
    match &callee_ty {
        Type::Nullable(_) | Type::Null | Type::Undefined | Type::Any => {
            for &a in args {
                let _ = checker.type_of(ast, a)?;
            }
            Ok(Type::Any)
        }
        _ => crate::check_type_of_call::check(checker, ast, eid, &callee, args),
    }
}

pub(crate) fn check_post_incr(
    checker: &mut Checker,
    ast: &Ast,
    target: ExprId,
) -> Result<Type, String> {
    let ty = checker.type_of(ast, target)?;
    if ty != Type::Number {
        return Err(format!(
            "post-increment requires a number target, got {ty:?}"
        ));
    }
    Ok(Type::Number)
}

pub(crate) fn check_sequence(
    checker: &mut Checker,
    ast: &Ast,
    left: ExprId,
    right: ExprId,
) -> Result<Type, String> {
    let _ = checker.type_of(ast, left)?;
    checker.type_of(ast, right)
}

pub(crate) fn check_as(
    checker: &mut Checker,
    ast: &Ast,
    expr: ExprId,
    ty_ann: &str,
) -> Result<Type, String> {
    let inner_ty = checker.type_of(ast, expr)?;
    if ty_ann == "__nonnull__" {
        return Ok(match inner_ty {
            Type::Nullable(inner) => (*inner).clone(),
            other => other,
        });
    }
    resolve_type_ann_full(ty_ann, &checker.aliases, &[], &checker.generic_alias_decls)
        .ok_or_else(|| format!("unknown cast target type `{ty_ann}`"))
}
