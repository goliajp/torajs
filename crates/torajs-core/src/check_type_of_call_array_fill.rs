//! `xs.fill(v [, start [, end]])` 1/2/3-arg Array-receiver
//! arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 231 — twenty-fourth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! V3-18 m1.h.53 — JS spec §22.1.3.6:
//! - `xs.fill(v)`         = `xs.fill(v, 0, len)`
//! - `xs.fill(v, start)`  = `xs.fill(v, start, len)`
//!
//! Pre-fix tora declared with 3 fixed params so 1/2-arg
//! calls hit the arity check; widen to `(1..=3)` here.
//!
//! - **S218** — accept `Undefined` for start/end per ES
//!   §23.1.3.7 step 5/9 (`ToIntegerOrInfinity(undefined)=0`
//!   for start, `end===undefined → len` for end). ssa_lower
//!   mirror short-circuits each undef slot to its spec
//!   default.
//! - **S335** — `xs.fill(v, Any [, Any])` per ES §23.1.3.7
//!   step 5/9: ToIntegerOrInfinity accepts arbitrary-typed
//!   input. Sister to S334 (Array.slice Any). ssa_lower
//!   mirror decodes via anyv_to_number → coerce_to_i64.
//! - **S0.4 (rotation 678)** — S218 / S335 named three
//!   admitted shapes and refused the rest, but
//!   ToIntegerOrInfinity refuses nothing:
//!   `[1,2,3].fill(9, true)` fills from 1 and
//!   `.fill(9, {valueOf(){return 2}})` from 2. The bound
//!   slots carry no shape gate at all now — only arg 0, the
//!   VALUE, is elem-typed.
//!
//! `Array<Any>.fill(v)` accepts a cross-type fill value —
//! ssa_lower routes through `arr_fill_any` which NaN-boxes
//! the value regardless of type. Mirror of S127-4 indexOf
//! 2-arg dedicated-arm Any-escape.
//!
//! Returns `Some(Ok(Array<T>))` on Array-receiver match;
//! `Some(Err(_))` on element-type mismatch or invalid
//! start/end type; `None` otherwise.

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
    if m_name != "fill" || !(1..=3).contains(&args.len()) {
        return None;
    }
    let src_ty = match crate::check::receiver_type(checker, ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Array(elem) = &src_ty else {
        return None;
    };
    let v_ty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    // An Any fill value into a Number/String elem admits (TS
    // any-assignability), paired with the lowering's shared
    // `coerce_push_value` unbox at the store boundary — the same
    // admit/coerce pairing as the push/unshift lane (rotation 158).
    // A function elem checks an Any value is a function, as push does.
    let any_into_scalar = matches!(v_ty, Type::Any)
        && (matches!(**elem, Type::Number | Type::String)
            || crate::check::face_slots::is_fn_elem(elem));
    crate::check::face_slots::record_elem_store(checker, elem, &v_ty);
    if v_ty != **elem && !matches!(**elem, Type::Any) && !any_into_scalar {
        return Some(Err(format!(
            "Array.fill arg 0 must match elem type {:?}, got {v_ty:?}",
            **elem
        )));
    }
    // §23.1.3.7 steps 4/6 are ToIntegerOrInfinity on start / end, and
    // that reaches every value — `[1,2,3].fill(9, true)` fills from 1,
    // `.fill(9, {valueOf(){return 2}})` from 2. Only arg 0 (the VALUE)
    // is elem-typed; the two bounds carry no shape gate. ssa_lower
    // mirrors this through `normalize_range_bound`.
    for a in args.iter().skip(1) {
        if let Err(e) = checker.type_of(ast, *a) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Array(Box::new((**elem).clone()))))
}
