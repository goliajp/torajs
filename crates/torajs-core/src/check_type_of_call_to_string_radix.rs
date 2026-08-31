//! `n.toString(radix?)` primitive method arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 218 — twelfth sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! Handles both BigInt and Number receivers — `.toString` on
//! either is a primitive method with optional radix in
//! [2, 36] that the standard `Type::Function` check rejects
//! for variable arity. Two separate `Expr::Member` arms:
//!
//! - **S247** — `BigInt.prototype.toString(radix, ...trailing)`
//!   trailing-arg ignore per ES §21.2.3.5. Spec reserves
//!   slots past the 1 useful radix but tora's helpers
//!   (`bigint_to_string` / `bigint_to_string_radix`) are
//!   1-arg only; trailing operand `type_of`'d for side
//!   effects then dropped at lower-time.
//!
//! - **S229 / S244** — `Number.toString(radix?, ...trailing)`.
//!   S229 accepts `Undefined` for radix per ES §21.1.3.6
//!   step 2-3 (undefined folds to 10, ssa_lower short-
//!   circuits to no-arg path); S244 accepts trailing args
//!   past the 1 useful radix slot (same shape as S238/S243).
//!
//! Returns `Some(Ok(String))` on a matched
//! `BigInt.toString` (≥ 2 args) or `Number.toString`
//! receiver; `Some(Err(_))` on bad radix type or arg
//! typecheck failure; `None` otherwise (other receivers
//! fall through to the regular method-table dispatch).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    // BigInt.prototype.toString(radix, ...trailing) — S247
    // trailing-arg ignore. Only fires for ≥ 2 args; the
    // 1-arg form keeps using the static-sig path.
    if let Expr::Member { obj, name } = ast.get_expr(*callee)
        && name == "toString"
    {
        let recv_ty = match checker.type_of(ast, *obj) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if recv_ty == Type::BigInt && args.len() >= 2 {
            for &aid in &args[1..] {
                if let Err(e) = checker.type_of(ast, aid) {
                    return Some(Err(e));
                }
            }
            // arg 0 still type_of'd via the Function sig's
            // first slot above; type_of args[0] here too
            // so any earlier-skipped inference fires.
            if let Err(e) = checker.type_of(ast, args[0]) {
                return Some(Err(e));
            }
            return Some(Ok(Type::String));
        }
    }
    // Number.toString(radix?, ...trailing) — S229 +
    // S244 combined.
    if let Expr::Member { obj, name } = ast.get_expr(*callee)
        && name == "toString"
    {
        let recv_ty = match checker.type_of(ast, *obj) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if recv_ty == Type::Number {
            if args.is_empty() {
                return Some(Ok(Type::String));
            }
            // Rotation 544 — §21.1.3.6 step 2 is
            // ToIntegerOrInfinity(radix), which reaches every value:
            // `(255).toString('16')` is "ff". The gate that stood
            // here admitted Number and Undefined and refused the
            // rest. `undefined` is NOT that coercion's NaN — it means
            // radix 10, while `toString(NaN)` is a RangeError — so
            // ssa_lower asks the any TAG rather than the number.
            if let Err(e) = checker.type_of(ast, args[0]) {
                return Some(Err(e));
            }
            for &aid in &args[1..] {
                if let Err(e) = checker.type_of(ast, aid) {
                    return Some(Err(e));
                }
            }
            return Some(Ok(Type::String));
        }
    }
    None
}
