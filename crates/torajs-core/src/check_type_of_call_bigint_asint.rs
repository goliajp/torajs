//! `BigInt.{asIntN,asUintN}(bits, value, ...trailing)`
//! BigInt-namespace 2-arg-spec trailing-arg ignore wedge
//! arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 276 — sixty-ninth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S314 — `BigInt.{asIntN,asUintN}(bits, value, ...trailing)`
//! per ES §21.2.2.{1,2}: spec sig is 2-arg; trailing args
//! MUST be silently ignored. Pre-S314 the fixed
//! (`Type::Object("BigInt")`, m) method-table sig rejected
//! `args.len() >= 3` with "expected 2 argument(s), got M".
//! typecheck-drop trailing + ssa_lower mirror widens gate
//! + lower-and-drop trailing.
//!
//! Rotation 463 — the wedge also took over the plain 2-arg form,
//! because `bits` needed to stop being shape-checked here. Step 1
//! of both algorithms is `ToIndex(bits)`, a COERCION followed by a
//! range test, so `BigInt.asIntN("-2.5", 0n)` is a RangeError
//! raised at runtime and not a compile-time refusal.
//!
//! Rotation 698 — `value` stopped being shape-checked for the same
//! reason. Step 2 is `ToBigInt(value)`, also a coercion: §7.1.13
//! converts a Boolean and a parseable String, runs an Object's
//! ToPrimitive, and raises a TypeError only for a Number / nullish /
//! Symbol. Refusing every non-`BigInt` shape here turned ten test262
//! files' worth of legal programs into compile errors
//! (`BigInt.asIntN(3, true)`, `(4, "0xa")`, `(4, [10n])`, an `any`
//! operand). The whole of §7.1.13 has been behind
//! `LowerCtx::coerce_any_to_bigint` since RFC 20260720 knife 5b-2,
//! and the lowering twin routes every non-`BigInt` operand through it.
//! A MISSING value slot is `undefined`, which is one of the TypeError
//! shapes — so the zero- and one-argument forms belong to this arm
//! too, instead of falling onto the arity gate.
//!
//! Receiver / m_name matrix:
//! - `Type::Object("BigInt")` + m_name ∈ {asIntN, asUintN}:
//!     - no args — §21.2.2.{1,2} run `ToIndex(undefined)` = 0 and
//!       then throw at step 2; `Some(Ok(Type::BigInt))`
//!     - args[0] (bits) — any shape; coerced by `ToIndex` at runtime
//!     - args[1] (value) — any shape; coerced by `ToBigInt`
//!     - args[2..] type_of'd for side-effect validation
//!     - returns `Some(Ok(Type::BigInt))`
//!
//! Returns:
//! - `Some(Ok(Type::BigInt))` on success
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, m_name not in
//!   {asIntN, asUintN}, or non-BigInt receiver — cascade falls
//!   through to the S309 Object.fromEntries wedge and beyond)

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &[ExprId],
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if !matches!(m_name.as_str(), "asIntN" | "asUintN") {
        return None;
    }
    let src_ty = match crate::check::receiver_type(checker, ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::Object("BigInt")) {
        return None;
    }
    if args.is_empty() {
        // `BigInt.asIntN()` — §21.2.2.1 step 1 is `ToIndex(undefined)`
        // = 0 (§7.1.22 gives ToIntegerOrInfinity(undefined) = 0, and
        // nothing observable runs), step 2 is `ToBigInt(undefined)`,
        // which is a TypeError. The answer is a runtime throw, not a
        // refusal, so this arm owns the shape instead of dropping it
        // on the arity gate.
        return Some(Ok(Type::BigInt));
    }
    let aty0 = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    // §21.2.2.{1,2} step 1 is `ToIndex(bits)` — a coercion, so no
    // operand shape is refused here; the range test happens at
    // runtime where the Number still is one. `let _ = aty0;` keeps
    // the type_of call, whose own errors stay loud.
    let _ = aty0;
    // §21.2.2.{1,2} step 2 is `ToBigInt(value)` — a coercion, so every
    // operand shape reaches it and the answer is whatever §7.1.13 says:
    // a Boolean or a parseable String converts, an Object runs its
    // ToPrimitive, a Number / nullish / Symbol is a TypeError. Refusing
    // the non-BigInt shapes here turned ten test262 files' worth of
    // legal programs into compile errors. The any-lane kernel behind
    // `coerce_any_to_bigint` has carried the whole of §7.1.13 since RFC
    // 20260720 knife 5b-2; the lowering twin routes through it.
    if let Some(&value_arg) = args.get(1)
        && let Err(e) = checker.type_of(ast, value_arg)
    {
        return Some(Err(e));
    }
    for &a in args.iter().skip(2) {
        if let Err(e) = checker.type_of(ast, a) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::BigInt))
}
