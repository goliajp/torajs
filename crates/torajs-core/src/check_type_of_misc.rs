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
    // `typeof C` on a type-only class is a value use too (TS1361);
    // the undeclared-ident forgiveness below must not swallow it
    if let Expr::Ident(name) = ast.get_expr(expr)
        && let Some(c) = type_only_class(ast, name)
    {
        return Err(type_only_value_error(c));
    }
    // V3-18 m1.h.3 / m1.h.20 — short-circuit known-builtin Ident
    // and Member-on-known-namespace; the spec result is the literal
    // string regardless of resolver state.
    if let Expr::Ident(name) = ast.get_expr(expr)
        && (checker.lookup(name).is_none()
            || is_known_builtin_global(name)
            || ast.module_foreign_reads.contains(&expr))
    {
        // §13.5.3 step 2 — `typeof` takes GetValue, and on a name with
        // no binding the lowering asks the global object at run time,
        // where an ACCESSOR runs its getter (which may throw). The ident
        // checker's own verdict marks the operand an undeclared read
        // when it truly is one (a function or global name resolves
        // there and takes no mark), and that mark is what the throw
        // analysis reads; the answer is String either way.
        if !is_known_builtin_global(name) {
            let _ = crate::check_type_of_ident::check(checker, ast, expr, name);
        }
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

/// ES §13.10.2 — `x instanceof T` answers Boolean whatever the
/// operands are.
///
/// The target is typechecked only when it is something larger than a
/// bare name. A bare name here is usually a CLASS, and a class is not
/// a value binding the checker can type — asking it for one would
/// refuse `x instanceof Foo`, the overwhelmingly common spelling.
/// Names keep being resolved where they always were: the lowering's
/// static ladder, which knows the class hierarchy. Anything larger IS
/// an ordinary value expression and is checked like one.
pub(crate) fn check_instanceof(
    checker: &mut Checker,
    ast: &Ast,
    expr: ExprId,
    rhs: ExprId,
) -> Result<Type, String> {
    let _ = checker.type_of(ast, expr)?;
    match ast.get_expr(rhs) {
        Expr::Ident(name) => {
            if let Some(c) = type_only_class(ast, name) {
                return Err(type_only_value_error(c));
            }
        }
        _ => {
            let _ = checker.type_of(ast, rhs)?;
        }
    }
    Ok(Type::Boolean)
}

/// The type-only class a value-position name reads — the bare
/// spelling, or the `__class_<C>` cell spelling the class-globals
/// pass rewrites a value use to.
pub(crate) fn type_only_class<'a>(ast: &Ast, name: &'a str) -> Option<&'a str> {
    let bare = name.strip_prefix("__class_").unwrap_or(name);
    ast.type_only_classes.contains(bare).then_some(bare)
}

/// TS1361 — a class that only an `import type` named has no value in
/// the program (its module never evaluated), so a value use of the
/// name is refused at compile time (`ast.type_only_classes`).
pub(crate) fn type_only_value_error(class: &str) -> String {
    format!("'{class}' cannot be used as a value because it was imported using 'import type'")
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
        // Rotation 362 — an Any lhs joins to ANY, not rhs's type: a
        // non-nullish lhs IS the result (§13.4.2), whatever it holds.
        // Answering rhs_ty made the lowering unbox the lhs to rhs's
        // register width, which read an f64 box's raw bits as an i64
        // (silent wrong) and collapsed non-bool payloads through a
        // != 0 test. The lowering boxes the rhs side instead.
        return Ok(Type::Any);
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
    // Rotation 507 — two DIFFERENT class instances meet at their
    // nearest common ancestor, the ternary's join (`m ?? new Leaf(1)`
    // with `m: Mid | null` used to reject where bun runs it). A
    // `Nullable` rhs unwraps first: the lhs already answers the
    // non-null side, so the join is over what each side can BE.
    let rhs_core = match &rhs_ty {
        Type::Nullable(i) => (**i).clone(),
        other => other.clone(),
    };
    if let Some(joined) = crate::check_type_compare::class_join(ast, &inner, &rhs_core) {
        return Ok(joined);
    }
    // §13.4.2 puts no condition on the two sides: the operator picks
    // one of them and the result is the union. TS spells that union
    // `NonNullable<L> | R`; this lattice has no general union, only
    // `Nullable(T)`, and until now everything it could not spell it
    // REFUSED — `o ?? "dflt"` on `o: { a: number } | undefined`, the
    // single most ordinary use of the operator this type exists for,
    // did not compile. Six of the seven spellings probed were refused.
    //
    // One of them the lattice CAN spell, so it stays typed rather
    // than being handed to the any lane: a `null` rhs makes the
    // result `T | null`, which is `Nullable(inner)` — the same
    // pointer-shaped slot, so the lowering is unchanged.
    //
    // An `undefined` rhs is NOT the same case, even though the
    // lattice would accept the same spelling for it: `Nullable`
    // collapses null and undefined into one pointer slot, so the
    // answer to `o ?? undefined` came back as `null`. It goes to the
    // any lane below, where the box keeps the two apart by tag.
    if matches!(rhs_ty, Type::Null) {
        return Ok(Type::Nullable(Box::new(inner)));
    }
    // Everything else is a union with no name here. `Any` is its
    // narrowest sound supertype in this lattice, and the lowering
    // widens both sides into it the way the `Any` lhs lane above
    // already does. This is not a typed path being given up — there
    // was no path: the alternative is refusing a program §13.4.2
    // fully specifies.
    let _ = inner;
    Ok(Type::Any)
}

pub(crate) fn check_opt_chain(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    obj: ExprId,
    name: &str,
) -> Result<Type, String> {
    let obj_ty = checker.type_of(ast, obj)?;
    // `a?.["k"]` is the chain spelling of the same fold the Member
    // lane marks (`Ast::index_spelled_props`), so a static miss is
    // lenient here for the same reason and answers `Any`. This
    // ladder is narrower than the one next door — it has drifted
    // from it before — so the leniency is spelled on both of its
    // `member_type` calls rather than inside it.
    let lenient = ast.index_spelled_props.contains(&eid);
    match &obj_ty {
        Type::Nullable(_) => {
            let inner_obj_ty = match &obj_ty {
                Type::Nullable(inner) => (**inner).clone(),
                _ => obj_ty.clone(),
            };
            if let Err(e) = checker.member_type(&inner_obj_ty, name)
                && !lenient
            {
                return Err(e);
            }
            Ok(Type::Any)
        }
        Type::Null | Type::Undefined | Type::Any => Ok(Type::Any),
        _ => {
            // Plain (non-nullable) obj: `?.` ≡ `.`. Return concrete
            // member type since the optional path is dead.
            match checker.member_type(&obj_ty, name) {
                Ok(t) => Ok(t),
                Err(_) if lenient => Ok(Type::Any),
                Err(e) => Err(e),
            }
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
    eid: ExprId,
    obj: ExprId,
    index: ExprId,
) -> Result<Type, String> {
    let obj_ty = checker.type_of(ast, obj)?;
    match &obj_ty {
        Type::Nullable(_) | Type::Null | Type::Undefined | Type::Any => {
            let _ = checker.type_of(ast, index)?;
            Ok(Type::Any)
        }
        _ => crate::check_type_of_index::check(checker, ast, eid, obj, index),
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
    // G2.5 — `globalThis.x++` is a member store in update-expression
    // clothing; same builtin-name gate as the assign / delete arms.
    match ast.get_expr(target) {
        crate::ast::Expr::Member { obj, name } => {
            crate::check_type_of_assign::reject_globalthis_mutation(
                checker,
                ast,
                *obj,
                Some(name.as_str()),
            )?;
            // RFC 20260915 #8 — a lifted `const` / `using` field.
            crate::check_assign_target::reject_const_lifted_update(checker, ast, target)?;
        }
        crate::ast::Expr::Index { obj, index } => {
            crate::check_type_of_assign::reject_globalthis_mutation(
                checker,
                ast,
                *obj,
                crate::check_type_of_assign::literal_index_key(ast, *index),
            )?;
        }
        _ => {}
    }
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
    // Both halves are visited before the left's verdict propagates —
    // `type_of` records into `expr_types` only on Ok, and a `?` here
    // left the right half untyped for lowering to read as a blank.
    let l = checker.type_of(ast, left);
    let r = checker.type_of(ast, right);
    l?;
    r
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
    let target = resolve_type_ann_full(ty_ann, &checker.aliases, &[], &checker.generic_alias_decls)
        .ok_or_else(|| format!("unknown cast target type `{ty_ann}`"))?;
    // a literal cast to a struct type is stored in that type's layout
    crate::check_assignable::in_declared_order(checker, ast, &target, expr, inner_ty);
    Ok(target)
}
