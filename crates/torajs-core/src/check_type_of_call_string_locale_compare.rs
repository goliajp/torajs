//! `s.localeCompare(thatStr, ...trailing)` String-receiver
//! 1+arg wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 258 — fifty-first sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S211 — String.localeCompare(undefined) per ES
//! §22.1.3.10 step 4: thatStr = ToString(thatValue) =
//! "undefined". Pre-fix declared `(String) -> Number`
//! rejected the typed-Undefined arg with
//! "argument 0: expected String, got Undefined". ssa_lower
//! inline-substitutes the interned "undefined" literal for
//! the typed-undefined operand.
//!
//! S238 — extend the same carve-out to the 2-arg (locales)
//! and 3-arg (locales, options) shapes per ES §22.1.3.10
//! trailing-arg ignore: the spec reserves those slots for
//! Intl-aware locale comparison but tora's bytewise helper
//! has no locale awareness, so they're ignored. The
//! ssa_lower_str loop trims any arg beyond i=0 so the
//! helper's (Str, Str) ABI never sees the trailing
//! operands.
//!
//! S285 — widen S238 carve-out `(1..=3)` → `>= 1` so 4+ arg
//! trailing-widen shape typechecks. ssa_lower mirror swaps
//! the loop `break i > 0` to `let _ = lower_expr(a);
//! continue` so step()-style side-effect exprs fire per ES
//! eval-then-discard (S272 idiom).
//!
//! Returns `Some(Ok(Type::Number))` when the receiver is
//! `Type::String` AND m_name == "localeCompare" AND
//! args.len() >= 1 AND args[0] ∈ {String, Undefined};
//! args[1..] type_of'd for side effects then dropped.
//! `Some(Err(_))` on recursive `type_of` failure (receiver
//! or any arg). `None` otherwise (non-Member callee,
//! m_name mismatch, empty args, non-String receiver, or
//! args[0] not in the allow-list — cascade falls through
//! to the keys / valueOf / pop-shift / general method
//! dispatch arms below).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if m_name != "localeCompare" || args.is_empty() {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    let aty0 = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(aty0, Type::String | Type::Undefined) {
        return None;
    }
    for &aid in &args[1..] {
        if let Err(e) = checker.type_of(ast, aid) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Number))
}
