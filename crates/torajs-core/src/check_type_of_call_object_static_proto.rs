//! `Object.{create,setPrototypeOf,defineProperties,
//! defineProperty}(obj, ..., ...trailing)` Object-namespace
//! trailing-arg ignore wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 267 — sixtieth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S269 — Object.{create,setPrototypeOf,defineProperties}
//! trailing-arg ignore per ES §20.1.2.{1,5,21}. tora's
//! fixed sigs (`vec![Type::Any]` for create / `vec![
//! Type::Any, Type::Any]` for the other two) rejected
//! the next arg; SSA-emit's intercept for all three
//! already eval-and-drops args[1..] (`for a in args`
//! / `for a in args.iter().skip(1)`), so the lower path
//! is safe — S269 widens checktime to accept the matching
//! floor and beyond.
//!
//! S317 — extend the same widen to `defineProperty(obj,
//! key, desc, ...trailing)` per ES §20.1.2.6. fixed
//! sig `vec![Type::Any, Type::String, Type::Any]` (3
//! args) rejected the 4th; paired ssa_lower change
//! widens `args.len() == 3` to `>= 3` + lowers-and-
//! drops args[3..] after `emit_define_one` for spec
//! left-to-right side-effect order.
//!
//! Per-method arity floor:
//! - `create` → 2 (proto + props-or-trailing)
//! - `setPrototypeOf` / `defineProperties` → 3 (target +
//!   second-arg + trailing-or-more)
//! - `defineProperty` → 4 (target + key + descriptor +
//!   trailing)
//!
//! Returns:
//! - `Some(Ok(Type::Void))` for {defineProperties,
//!   defineProperty} success
//! - `Some(Ok(Type::Any))` for {create, setPrototypeOf}
//!   success
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, m_name not in
//!   the allowlist, non-Object("Object") receiver, or
//!   arity below floor — cascade falls through to the
//!   Date setter / general method dispatch arms below)

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
        "create" | "setPrototypeOf" | "defineProperties" | "defineProperty"
    ) {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::Object("Object")) {
        return None;
    }
    let floor: usize = match m_name.as_str() {
        "create" => 2,
        "setPrototypeOf" | "defineProperties" => 3,
        "defineProperty" => 4,
        _ => unreachable!(),
    };
    if args.len() < floor {
        return None;
    }
    for &arg in args.iter() {
        if let Err(e) = checker.type_of(ast, arg) {
            return Some(Err(e));
        }
    }
    // ES §20.1.2.5 / §20.1.2.6 — `Object.defineProperty` and
    // `defineProperties` both return O (the receiver), not `undefined`.
    // Pre-fix these two arms answered `Type::Void`, which forbade
    // fixtures like `let root = Object.defineProperty({}, ...)` from
    // typechecking OR made them assign `undefined` — see the mirror
    // fix in ssa_lower_object_define.rs::try_lower_define_property.
    Some(Ok(Type::Any))
}
