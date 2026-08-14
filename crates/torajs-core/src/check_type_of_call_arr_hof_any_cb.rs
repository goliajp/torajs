//! `Array<T>.{map,filter,…}(cb, …)` where the CALLBACK types `any` —
//! route the whole call to the runtime any-method lane (398-10).
//!
//! The method-table arm spells the callback slot as a concrete
//! `(T, number, T[]) => R` and the general tail's as-cast wedge
//! re-runs the type UNDER the cast against it, so both spellings —
//! a bare `any` binding and `cb as any` — died at "argument 0:
//! expected Function(…), got Any". TS says `any` absorbs the slot;
//! bun runs the call.
//!
//! Admitting here is only half the fix, and the half that MUST NOT
//! ship alone: the inline typed loop needs the callback's static
//! signature, so an admitted-but-typed-lowered call would misread
//! every element (rotation 400 knife 5 measured exactly this shape
//! of silent wrong on the ternary join). Instead the call leaves the
//! typed tier entirely: recording the callee as `Any` is what the
//! lowering side keys on (`ssa_lower_any_method_call`'s cluster-#4
//! branch — the RFC 20260806 blade-2 pairing), so the receiver boxes
//! at that boundary and the runtime `arr_method_callback` kernels
//! run the walk (`__torajs_arr_any_map` and friends — thisArg + the
//! `FLAG_CLOSURE_RECV_FIRST` argv shift included). The result types
//! `Any` for the same reason: the kernels produce `Arr<Any>`
//! products, and a typed reader over those slots would see NaN-box
//! bits.
//!
//! The allowlist is exactly the method set the any-lane
//! `arr_method_callback` dispatcher serves MINUS `sort`: sort
//! mutates the receiver in place, and an in-place any-lane write
//! back into a typed `Arr<T>` block is an unaudited repr boundary
//! (loud reject keeps it visible). `flatMap` / `findLast` /
//! `findLastIndex` / `toSorted` are not in that dispatcher's
//! callback set and keep the loud reject too.

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
    if !matches!(
        name.as_str(),
        "map"
            | "filter"
            | "forEach"
            | "every"
            | "some"
            | "find"
            | "findIndex"
            | "reduce"
            | "reduceRight"
    ) || args.is_empty()
    {
        return None;
    }
    let Ok(obj_ty) = checker.type_of(ast, *obj) else {
        return None;
    };
    if !matches!(obj_ty, Type::Array(_)) {
        return None;
    }
    let cb_ty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(_) => return None,
    };
    if cb_ty != Type::Any {
        return None;
    }
    // Type the rest of the arguments (a thisArg / reduce init) — the
    // walk both surfaces their own errors and records the call sites
    // an implicit generic is instantiated from.
    for a in &args[1..] {
        if let Err(e) = checker.type_of(ast, *a) {
            return Some(Err(e));
        }
    }
    checker.expr_types.insert(*callee, Type::Any);
    Some(Ok(Type::Any))
}
