//! `Math.<unary/binary>(Any[, Any])` widen arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 242 — thirty-fifth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S342 — `Math.<unary/binary>(Any[, Any])` per ES §21.3.2.*:
//! every `Math` method that takes Number args applies
//! `ToNumber`, which accepts arbitrary-typed input. The
//! method-table sigs `vec![Type::Number]` /
//! `vec![Type::Number, Type::Number]` at the regular Math
//! dispatch site strict-rejected `o: any` operands. Widen
//! here so the ssa_lower mirror routes `Any` through
//! `anyv_to_number → F64 → Math helper`.
//!
//! S0.4 (rotation 678) — there is no per-arg check any more.
//! ToNumber refuses nothing, so `String` / `Bool` / `Object`
//! belong here with `Any`; ssa_lower's
//! [`crate::ssa_lower::LowerCtx::coerce_to_f64_tonumber`] is the
//! one road that serves all of them. An all-Number call still
//! falls through, because the strict method-table is where this
//! lane's ARITY is settled.
//!
//! `min` / `max` never reach here at all: `route_globals` runs
//! before this route and its `math_min_max` arm claims every
//! arity of those two.
//!
//! Returns `Some(Ok(Type::Number))` when at least one arg is not
//! statically `Number`. `Some(Err(_))` only if recursive
//! `type_of` on an arg fails.
//! `None` for non-`Math.*` callee, non-matched method name,
//! or all-Number args (let the strict table handle it).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    let Expr::Ident(ns) = ast.get_expr(*ns_id) else {
        return None;
    };
    if ns != "Math"
        || !matches!(
            m_name.as_str(),
            "sqrt"
                | "abs"
                | "floor"
                | "ceil"
                | "log"
                | "exp"
                | "sign"
                | "round"
                | "trunc"
                | "sin"
                | "cos"
                | "tan"
                | "asin"
                | "acos"
                | "atan"
                | "log2"
                | "log10"
                | "cbrt"
                | "sinh"
                | "cosh"
                | "tanh"
                | "asinh"
                | "acosh"
                | "atanh"
                | "expm1"
                | "log1p"
                | "fround"
                | "f16round"
                | "pow"
                | "min"
                | "max"
                | "atan2"
        )
    {
        return None;
    }
    // §21.3.2.* step 1 on every Math method that declares Number
    // params is ToNumber, and ToNumber reaches EVERY value:
    // `Math.abs({valueOf(){return -7}})` is 7 and `Math.abs({})` is
    // NaN, neither a type error. S342 named `Any` and let every other
    // non-Number shape fall through to the strict method-table gate,
    // which refuses — so one spelling of a value answered while
    // another died at lower time. The per-arg question is gone; what
    // remains is only WHICH arm claims the call.
    //
    // An all-Number call still falls through, because the strict table
    // is also where this lane's arity is settled: claiming those here
    // would take `Math.abs()` away from `math_unary_0arg` and
    // `Math.abs(1, 2)` away from the trailing-ignore arm.
    // Arity stays exactly where it was. This arm has never checked it
    // -- it did not need to, because it only ever claimed calls whose
    // args were already `Number | Any`, and a wrong-arity one of those
    // fell through to the strict method-table. Now that it claims
    // every shape, it has to say which arities it is claiming, or
    // `Math.abs(o, 2)` would arrive at a lowering whose argv walk
    // asserts one operand.
    let arity_ok = match m_name.as_str() {
        "pow" | "atan2" => args.len() == 2,
        // NOT `imul`, and NOT `clz32` -- §21.3.2.{19,11} read their
        // arguments with ToUint32, and `__torajs_math_imul` /
        // `__torajs_math_clz32` take `(i64, i64)` / `(i64)`. They are
        // not on this road at all, and `is_math_unary` /
        // `is_math_binary` -- the f64 ABI the coercion below feeds --
        // exclude them for the same reason, so a widened call site
        // hands a boxed operand to an integer slot. Widening `imul`
        // answered 12486576 for `Math.imul(o, 3)`; widening `clz32`
        // turned `staging/sm/Math/clz32` from a refusal into a wrong
        // answer, which is worse. Their family is the int32 sink and
        // it needs its own knife.
        //
        // Every other name in the list above IS in `is_math_unary` or
        // `is_math_binary`; that is the membership test to redo
        // whenever a name is added here.
        "imul" => false,
        // `route_globals` runs first and its `math_min_max` arm claims
        // every arity of these two, so this is unreachable today --
        // written down so a route reorder cannot make it silent.
        "min" | "max" => false,
        _ => args.len() == 1,
    };
    if !arity_ok {
        return None;
    }
    let mut coerced_seen = false;
    for &aid in args {
        let aty = match checker.type_of(ast, aid) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if aty != Type::Number {
            coerced_seen = true;
        }
    }
    if coerced_seen {
        return Some(Ok(Type::Number));
    }
    None
}
