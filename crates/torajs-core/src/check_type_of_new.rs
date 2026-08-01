//! `Expr::New { class_name, args }` typecheck for built-in classes
//! pulled out of [`crate::check::Checker::type_of_inner`]'s 8 `New`
//! sub-arms as chunk-92 of the type_of_inner decomp.
//!
//! Built-in receivers handled here (user classes are pre-rewritten
//! by `desugar_classes`):
//!
//! - **`new WeakRef(target)`** (T-26) — exactly 1 arg; target type
//!   not constrained (any heap-shaped value). Returns
//!   `Type::WeakRef`.
//! - **`new WeakMap()` / `new WeakSet()`** (T-26.B) — zero-arg
//!   only; iterable initializer overload deferred.
//! - **`new Map([[k,v], ...])` / `new Map(<Map>|<Array<Array<T>>>)`**
//!   (P6.1 / S133-5 / S156 / S166) — static array-of-pair literal
//!   ssa-lower unrolls to `map_create + map_set`; runtime
//!   `Array<Array<T>>` / Map copy-ctor go through helper intrinsic;
//!   zero-arg also valid. Returns `Type::Map`.
//! - **`new Set([a, b, c])` / `new Set(<Set>|<Array<T>>)`**
//!   (P6.1 / S133-4 / S152 / S166) — analogous Set initializer.
//!   Returns `Type::Set`.
//! - **`new Array(n)`** (P0.10) — 1-arg numeric form per ES
//!   §23.1.2.1. Returns `Array<Any>`; 0-arg and ≥2-arg forms are
//!   pre-rewritten to array literals by `desugar_builtin_new`.
//! - **`new RegExp(pat, flags?)`** (§22.2.3.1) — dynamic-arg form
//!   (static-string-literal shapes are pre-rewritten to
//!   `Expr::Regex`). Returns `Type::RegExp`. Narrow ship: requires
//!   String args (ToString coercion for non-string args is a
//!   follow-up).
//! - **fallback** — `panic!` (user classes should have been
//!   desugared into `__new_<C>(…)` Call before reaching here).
//!
//! Returns `Some` when the class_name matches a built-in. Falls
//! through (`None`) for any unhandled class name; the caller's
//! catch-all arm produces the panic.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};
use crate::check_assignable::is_assignable_to_resolved;

pub(crate) fn try_check(
    checker: &mut Checker,
    ast: &Ast,
    class_name: &str,
    args: &[ExprId],
) -> Option<Result<Type, String>> {
    let result = match class_name {
        "WeakRef" => check_weak_ref(checker, ast, args),
        "WeakMap" => check_weak_map(args),
        "WeakSet" => check_weak_set(args),
        "Map" => check_map(checker, ast, args),
        "Set" => check_set(checker, ast, args),
        "Array" => check_array(checker, ast, args),
        "RegExp" => check_regexp(checker, ast, args),
        "Number" => check_number_wrapper(checker, ast, args),
        "String" => check_string_wrapper(checker, ast, args),
        "Boolean" => check_boolean_wrapper(checker, ast, args),
        // RFC 20260730-iterator-global 刀 1 — `new Iterator()` types
        // Any; the lowering emits the §27.1.3.1 abstract-ctor
        // TypeError (runtime, catchable).
        "Iterator" => Ok(Type::Any),
        _ => return None,
    };
    Some(result)
}

/// RFC 20260716 刀 2 — `new Number(x)` constructs a NumberWrapper heap
/// cell (`[[NumberData]] = ToNumber(x)`, ES §21.1.1.1). No dedicated
/// `Type::NumberWrapper` variant yet — checker returns `Type::Any`
/// (heap identity + tag walkers do the right thing at ssa_lower / any
/// lane; later blades add a nominal Type once member ladder + auto-
/// unbox land). 0-arg is pre-desugared to primitive `0` by
/// `desugar_builtin_new`; trailing args typechecked-and-dropped per
/// spec.
fn check_number_wrapper(checker: &mut Checker, ast: &Ast, args: &[ExprId]) -> Result<Type, String> {
    if args.is_empty() {
        return Err(
            "internal: `new Number()` with 0 args reached check.rs (desugar didn't run?)".into(),
        );
    }
    let arg_ty = checker.type_of(ast, args[0])?;
    if !matches!(
        arg_ty,
        Type::Number
            | Type::Boolean
            | Type::Null
            | Type::Undefined
            | Type::String
            | Type::BigInt
            | Type::Any
            | Type::Array(_)
    ) {
        return Err(format!(
            "`new Number({arg_ty:?})`: primitive-coercible arg not yet supported \
             (RFC 20260716 刀 2 follow-up)"
        ));
    }
    for &arg in args.iter().skip(1) {
        let _ = checker.type_of(ast, arg)?;
    }
    Ok(Type::Any)
}

/// RFC 20260716 刀 2c — `new Boolean(x)` constructs a BooleanWrapper
/// heap cell (`[[BooleanData]] = ToBoolean(x)`, ES §20.3.1.1). Same
/// Type::Any return + Any-lane dispatch strategy as the Number /
/// String wrapper arms. `ToBoolean` accepts every value in the type
/// lattice, so the arg-type check just runs `type_of` for side
/// effects (and to reject upstream errors).
fn check_boolean_wrapper(
    checker: &mut Checker,
    ast: &Ast,
    args: &[ExprId],
) -> Result<Type, String> {
    if args.is_empty() {
        return Err(
            "internal: `new Boolean()` with 0 args reached check.rs (desugar didn't run?)".into(),
        );
    }
    let _ = checker.type_of(ast, args[0])?;
    for &arg in args.iter().skip(1) {
        let _ = checker.type_of(ast, arg)?;
    }
    Ok(Type::Any)
}

/// RFC 20260716 刀 2b — `new String(x)` constructs a StringWrapper
/// heap cell (`[[StringData]] = ToString(x)`, ES §22.1.1.1). Same
/// Type::Any return + Any-lane dispatch strategy as
/// `check_number_wrapper` above (no nominal `Type::StringWrapper`
/// yet; later blades add one). 0-arg is pre-desugared to `""`;
/// trailing args typechecked-and-dropped per spec.
fn check_string_wrapper(checker: &mut Checker, ast: &Ast, args: &[ExprId]) -> Result<Type, String> {
    if args.is_empty() {
        return Err(
            "internal: `new String()` with 0 args reached check.rs (desugar didn't run?)".into(),
        );
    }
    let arg_ty = checker.type_of(ast, args[0])?;
    // Same coercion lattice as the callable `String(x)` (see
    // check_type_of_call_global_ctors) — ToString accepts every
    // primitive plus Any / Array / Struct.
    if !matches!(
        arg_ty,
        Type::Number
            | Type::Boolean
            | Type::Null
            | Type::Undefined
            | Type::String
            | Type::BigInt
            | Type::Any
            | Type::Array(_)
            | Type::Struct(_)
    ) {
        return Err(format!(
            "`new String({arg_ty:?})`: primitive-coercible arg not yet supported \
             (RFC 20260716 刀 2b follow-up)"
        ));
    }
    for &arg in args.iter().skip(1) {
        let _ = checker.type_of(ast, arg)?;
    }
    Ok(Type::Any)
}

fn check_weak_ref(checker: &mut Checker, ast: &Ast, args: &[ExprId]) -> Result<Type, String> {
    if args.len() != 1 {
        return Err(format!(
            "`new WeakRef(...)` requires exactly 1 argument, got {}",
            args.len()
        ));
    }
    let _ = checker.type_of(ast, args[0])?;
    Ok(Type::WeakRef)
}

fn check_weak_map(args: &[ExprId]) -> Result<Type, String> {
    if !args.is_empty() {
        return Err(format!(
            "`new WeakMap(...)` with initializer not yet supported (got {} args)",
            args.len()
        ));
    }
    Ok(Type::WeakMap)
}

fn check_weak_set(args: &[ExprId]) -> Result<Type, String> {
    if !args.is_empty() {
        return Err(format!(
            "`new WeakSet(...)` with initializer not yet supported (got {} args)",
            args.len()
        ));
    }
    Ok(Type::WeakSet)
}

fn check_map(checker: &mut Checker, ast: &Ast, args: &[ExprId]) -> Result<Type, String> {
    // S133-5 — `new Map([[k, v], ...])` static array-literal-of-
    // pair-arrays initializer. ssa-lower unrolls to map_create +
    // map_set per pair.
    if args.len() == 1
        && let Expr::Array(elems) = ast.get_expr(args[0])
        && elems.iter().all(|e| {
            if matches!(ast.get_expr(*e), Expr::Spread { .. }) {
                return false;
            }
            if let Expr::Array(pair) = ast.get_expr(*e) {
                pair.len() == 2
                    && pair
                        .iter()
                        .all(|p| !matches!(ast.get_expr(*p), Expr::Spread { .. }))
            } else {
                false
            }
        })
    {
        for pair_eid in elems {
            if let Expr::Array(pair) = ast.get_expr(*pair_eid) {
                let _ = checker.type_of(ast, pair[0])?;
                let _ = checker.type_of(ast, pair[1])?;
            }
        }
        return Ok(Type::Map);
    }
    // S156 / S166 — runtime-shaped Array<Array<T>> or Map copy-ctor.
    if args.len() == 1 {
        let arg_ty = checker.type_of(ast, args[0])?;
        if matches!(arg_ty, Type::Map) {
            return Ok(Type::Map);
        }
        if let Type::Array(inner) = &arg_ty
            && matches!(**inner, Type::Array(_))
        {
            return Ok(Type::Map);
        }
    }
    if !args.is_empty() {
        return Err(format!(
            "`new Map(...)` with iterable initializer not yet supported (got {} args)",
            args.len()
        ));
    }
    Ok(Type::Map)
}

fn check_set(checker: &mut Checker, ast: &Ast, args: &[ExprId]) -> Result<Type, String> {
    // S133-4 — `new Set([a, b, c])` static array-literal.
    if args.len() == 1
        && let Expr::Array(elems) = ast.get_expr(args[0])
        && elems
            .iter()
            .all(|e| !matches!(ast.get_expr(*e), Expr::Spread { .. }))
    {
        for elem in elems {
            let _ = checker.type_of(ast, *elem)?;
        }
        return Ok(Type::Set);
    }
    // S152 / S166 — runtime-shaped Array<T> or Set copy-ctor.
    if args.len() == 1 {
        let arg_ty = checker.type_of(ast, args[0])?;
        if matches!(arg_ty, Type::Set) {
            return Ok(Type::Set);
        }
        if matches!(arg_ty, Type::Array(_)) {
            return Ok(Type::Set);
        }
    }
    if !args.is_empty() {
        return Err(format!(
            "`new Set(...)` with iterable initializer not yet supported (got {} args)",
            args.len()
        ));
    }
    Ok(Type::Set)
}

fn check_array(checker: &mut Checker, ast: &Ast, args: &[ExprId]) -> Result<Type, String> {
    if args.len() == 1 {
        let arg_ty = checker.type_of(ast, args[0])?;
        if !is_assignable_to_resolved(
            &Type::Number,
            &arg_ty,
            &checker.class_structs,
            &checker.aliases,
            &checker.generic_alias_decls,
        ) {
            return Err(format!(
                "`new Array(...)` 1-arg form: arg must be number, got {arg_ty:?}"
            ));
        }
        Ok(Type::Array(Box::new(Type::Any)))
    } else {
        Err(format!(
            "internal: `new Array(...)` with {} args reached check.rs (desugar didn't run?)",
            args.len()
        ))
    }
}

fn check_regexp(checker: &mut Checker, ast: &Ast, args: &[ExprId]) -> Result<Type, String> {
    if args.is_empty() || args.len() > 2 {
        return Err(format!(
            "`new RegExp(...)` requires 1 or 2 arguments, got {}",
            args.len()
        ));
    }
    // §22.2.3.1 — the pattern domain is the whole value domain (a
    // RegExp pattern copies source/flags per step 5, everything else
    // runs ToString per step 6; the compile_any kernel dispatches at
    // run time), and undefined flags default to the pattern's. The
    // old String-only gate predates the RegExp(...)-as-function
    // rewrite (rotation 267); it survives only as the argument-count
    // check above.
    for arg in args {
        checker.type_of(ast, *arg)?;
    }
    Ok(Type::RegExp)
}
