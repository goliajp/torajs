//! `Array.prototype.{splice,toSpliced}` — the two bounds are a
//! ToIntegerOrInfinity family slot, so the shape gate comes off.
//!
//! §23.1.3.31 step 3 puts ToIntegerOrInfinity on `start`, and step 7
//! puts it on `deleteCount`; §23.1.3.34 does the same for `toSpliced`.
//! That operation refuses no value, so neither slot has a shape to
//! gate on -- `x.splice({valueOf(){return 0}}, 3)` is `x.splice(0, 3)`
//! and `x.splice(undefined, undefined)` removes nothing.
//!
//! The road was already built on the lowering side: `emit_splice_return`
//! reads both slots through `lower_to_index_operand`, and it tells an
//! explicit `undefined` deleteCount (spec: 0) from an omitted one
//! (spec: `len - actualStart`) by asking `expr_types`, not by counting
//! arguments. Only the checker still refused, through the strict
//! method table, which is why `x.splice(1, undefined)` ran while
//! `x.splice(undefined, undefined)` did not: the arity-narrow wedge
//! [`crate::check_type_of_call_array_splice_2arg_undef`] had widened
//! arg 1 and nothing had widened arg 0. Half of a rule, again.
//!
//! Scope is `args.len() <= 2` on purpose. From arg 2 on, splice takes
//! ITEMS, which are element-typed and keep their gate in
//! [`crate::check_type_of_call_array_splice_insert`] -- a bound and an
//! element are different questions and must not share an arm.
//!
//! An all-Number call returns `None` and falls through to the path it
//! has always taken, so nothing that already worked changes hands.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &[ExprId],
) -> Option<Result<Type, String>> {
    let Expr::Member { obj, name } = ast.get_expr(*callee) else {
        return None;
    };
    if !matches!(name.as_str(), "splice" | "toSpliced") {
        return None;
    }
    if args.is_empty() || args.len() > 2 {
        return None;
    }
    let recv_ty = match checker.type_of(ast, *obj) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Array(elem) = &recv_ty else {
        return None;
    };
    let elem = (**elem).clone();
    let mut coerced_seen = false;
    for &a in args {
        let aty = match checker.type_of(ast, a) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if aty != Type::Number {
            coerced_seen = true;
        }
    }
    if !coerced_seen {
        return None;
    }
    Some(Ok(Type::Array(Box::new(elem))))
}
