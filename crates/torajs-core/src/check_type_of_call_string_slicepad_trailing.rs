//! `s.{slice,substring,substr,padStart,padEnd}(a, b, ...trailing)`
//! String-receiver trailing-arg ignore wedge arm extracted
//! from [`crate::check_type_of_call::check`]'s top-level
//! cascade (chunk 257 — fiftieth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S241 — String.{slice,substring,substr,padStart,padEnd}
//! (a, b, ...trailing) trailing-arg ignore per ES
//! §22.1.3.{20,22,23,16,17}: spec reserves slots past the
//! 2 useful args (start/end / start/length / maxLen/fillStr)
//! but tora's helpers are 2-arg only. Trailing operand
//! type_of'd for side effects then dropped at lower-time
//! (ssa_lower break early past i=1). Same shape as S238
//! localeCompare.
//!
//! S284 — widen from `args.len() == 3` (single trailing)
//! to `args.len() >= 3` (any trailing count). ssa_lower
//! mirror swaps the loop break to lower_expr + continue
//! so step()-style side-effect exprs fire per ES eval-
//! then-discard semantics (S272 idiom).
//!
//! Returns `Some(Ok(Type::String))` when the receiver is
//! `Type::String` AND m_name is one of the 5 supported
//! methods AND args.len() >= 3 AND arg 0 / arg 1 match the
//! per-method type allow-list:
//!   slice / substring → arg0,arg1 ∈ {Number, Undefined}
//!   substr            → arg0,arg1 ∈ {Number}
//!   padStart / padEnd → arg0 ∈ {Number, Undefined},
//!                       arg1 ∈ {String, Undefined}
//! Trailing args (args[2..]) are type_of'd for side-effect
//! validation only. `Some(Err(_))` on per-arg mismatch
//! (exhaustive once gated to String) or recursive
//! `type_of` failure. `None` otherwise (non-Member callee,
//! m_name mismatch, arity < 3, or non-String receiver —
//! cascade falls through to the localeCompare / general
//! method dispatch arms below).

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
    if !matches!(
        m_name.as_str(),
        "slice" | "substring" | "substr" | "padStart" | "padEnd"
    ) || args.len() < 3
    {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    // Rotation 545 — the numeric slots (arg 0 everywhere, arg 1 for
    // the slice family) run ToIntegerOrInfinity / ToLength: coerced,
    // not shape-checked, so the checker only typechecks and the
    // numslot lowering owns the shape dispatch (rotation 463
    // charCodeAt precedent). The pad fill slot stays gated — it is a
    // ToString slot, a different family.
    if let Err(e) = checker.type_of(ast, args[0]) {
        return Some(Err(e));
    }
    let aty1 = match checker.type_of(ast, args[1]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let arg1_ok = match m_name.as_str() {
        "slice" | "substring" | "substr" => true,
        "padStart" | "padEnd" => matches!(aty1, Type::String | Type::Undefined),
        _ => false,
    };
    if !arg1_ok {
        return Some(Err(format!(
            "String.{m_name} arg 1 type mismatch, got {aty1:?}"
        )));
    }
    for &a in &args[2..] {
        if let Err(e) = checker.type_of(ast, a) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::String))
}
