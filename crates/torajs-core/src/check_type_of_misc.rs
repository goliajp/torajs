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

/// ES §13.5.1 — `delete <Member|Index>` → Boolean. The parser only
/// admits property-reference operands; here the receiver must be
/// `any`-typed (the dynobj / expando world where OrdinaryDelete is
/// meaningful) and an Index key must be one the property-key domain
/// can name. Only the receiver and key are typechecked, never the
/// property itself: deleting an absent key is legal and answers true.
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
///
/// Out, and not for want of a lowering:
///
/// - An array of UNBOXED elements (`number[]`, `string[]`). The kernel
///   is reached but the hole write is refused at run time as an
///   element-kind change, which is exactly right: those slots have no
///   spare value. Admitting it would trade a compile-time refusal for a
///   run-time throw.
/// - A Struct. Its declared members are fixed layout slots with nowhere
///   to go, so the kernel answers false and removes nothing (bun
///   deletes and answers true — the divergence recorded in r312).
///   Admitting it would turn a whole-program refusal into a program
///   that runs and answers wrongly, which the design principles rank
///   strictly worse.
///
/// Both gates lift together when a slot can express absence, not before.
fn receiver_admits_delete(obj_ty: &Type) -> bool {
    match obj_ty {
        Type::Any => true,
        Type::Array(elem) => **elem == Type::Any,
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
        crate::ast::Expr::Member { obj, .. } => {
            let obj_ty = checker.type_of(ast, *obj)?;
            if receiver_admits_delete(&obj_ty) {
                Ok(Type::Boolean)
            } else {
                Err(delete_receiver_error(&obj_ty))
            }
        }
        crate::ast::Expr::Index { obj, index } => {
            let obj_ty = checker.type_of(ast, *obj)?;
            let idx_ty = checker.type_of(ast, *index)?;
            if !receiver_admits_delete(&obj_ty) {
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
            // BigInt is deliberately still out: that coercer appends the
            // `n` suffix (it serves BigInt *printing*), so it would name
            // an entry no property has and delete would answer true
            // having removed nothing — silent-wrong, which is worse than
            // this loud refusal.
            if !matches!(
                idx_ty,
                Type::String | Type::Symbol | Type::Any | Type::Number
            ) {
                return Err(format!(
                    "`delete` key must be a string, number or symbol (got {idx_ty:?})"
                ));
            }
            Ok(Type::Boolean)
        }
        _ => Err("`delete` target must be a property reference (obj.k / obj[k])".into()),
    }
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
    // RFC 20260730-undeclared-ident, write position — §13.4.4.1 step 1
    // is GetValue on the target, so an update expression over an
    // unresolvable name raises the READ-side ReferenceError before any
    // write happens. The type_of above marked the target; keep the mark
    // (the post-incr lowering lane consults it by target eid and emits
    // the throw) and answer Any — the value lane past the throw is
    // unreachable.
    if checker.undeclared_reads.contains_key(&target) {
        return Ok(Type::Any);
    }
    // ES §13.4.4.1 step 1 is ToNumeric, so an update expression is
    // legal over any value — `"5"++` is 6, `true++` is 2. The typed
    // lane still demands a Number because it adds one at the slot's
    // own width with no coercion in between; an `any` slot carries
    // the coercion into the runtime step and answers `any` (the
    // result is a Number or, for a BigInt operand, a BigInt).
    match ty {
        Type::Number => Ok(Type::Number),
        Type::Any => Ok(Type::Any),
        other => Err(format!(
            "post-increment requires a number target, got {other:?}"
        )),
    }
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
