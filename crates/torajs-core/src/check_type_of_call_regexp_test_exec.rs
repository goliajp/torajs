//! `r.{test,exec,toString}(s?, ...trailing)` RegExp-receiver
//! trailing-arg ignore wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 270 — sixty-third sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S266 — RegExp.{test,exec,toString}(s?, ...trailing)
//! per ES §22.2.6.{2,7,16}. Each method's fixed sig
//! (`vec![Type::String]` / `Vec::new()`) rejected 1+
//! trailing args. SSA-emit uses only args[0] (test/exec)
//! or no args (toString); the matching widen relaxes
//! the debug-assert + eval-and-drops args[1..].
//!
//! Per-method arity gate:
//! - `test` / `exec` → args.len() >= 2 (1 useful + trailing)
//! - `toString` → !args.is_empty() (0 useful + trailing)
//!
//! Returns:
//! - `Some(Ok(Type::Boolean))` for test
//! - `Some(Ok(Type::Array<String>))` for exec
//! - `Some(Ok(Type::String))` for toString
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, m_name not in
//!   {test, exec, toString}, non-RegExp receiver, or
//!   arity below per-method gate — cascade falls through
//!   to the Object.getPrototypeOf / general method
//!   dispatch arms below)

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
    if !matches!(m_name.as_str(), "test" | "exec" | "toString") {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if matches!(src_ty, Type::RegExp)
        && matches!(m_name.as_str(), "test" | "exec")
        && args.len() >= 2
    {
        for &arg in args.iter() {
            if let Err(e) = checker.type_of(ast, arg) {
                return Some(Err(e));
            }
        }
        return Some(Ok(match m_name.as_str() {
            "test" => Type::Boolean,
            "exec" => Type::Array(Box::new(Type::String)),
            _ => unreachable!(),
        }));
    }
    if matches!(src_ty, Type::RegExp) && m_name == "toString" && !args.is_empty() {
        for &arg in args.iter() {
            if let Err(e) = checker.type_of(ast, arg) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::String));
    }
    None
}
