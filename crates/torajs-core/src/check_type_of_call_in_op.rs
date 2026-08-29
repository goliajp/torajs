//! T-45 — `__torajs_in_op(key, obj)` synthetic call typecheck
//! extracted from [`crate::check_type_of_call::check`]'s
//! top-level cascade (chunk 296 — eighty-eighth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! The parser lowers a binary `in` operator (`key in obj`)
//! into a synthetic call `__torajs_in_op(key, obj)` so the
//! type checker sees a uniform `Expr::Call` and ssa_lower
//! can intercept by name to emit the type-dispatched
//! membership check. This wedge gates the 2-arg shape, runs
//! `type_of` on both operands for side effects and returns
//! `Type::Boolean` unconditionally. What the rhs may BE is not
//! this wedge's question: §13.10.1 step 5 makes a non-Object rhs a
//! runtime TypeError.
//!
//! Returns:
//! - `Some(Ok(Type::Boolean))` — the synthetic call typechecks
//!   (both operand `type_of` calls succeed)
//! - `Some(Err(_))` on either operand `type_of` failure
//! - `None` otherwise (non-Ident callee, callee name not
//!   `__torajs_in_op`, or args.len() != 2 — cascade falls
//!   through to the Promise.then / global-ctor siblings)

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Ident(n) = ast.get_expr(*callee) else {
        return None;
    };
    if n != "__torajs_in_op" || args.len() != 2 {
        return None;
    }
    if let Err(e) = checker.type_of(ast, args[0]) {
        return Some(Err(e));
    }
    // RFC 20260715-nominal-class-identity — `in` asks a STRUCTURAL
    // question ("does this shape carry the key"), so unwrap a class
    // instance's name to the struct behind it.
    let obj_ty = match checker.type_of(ast, args[1]) {
        Ok(t) => crate::check::resolve_class_ref(
            &t,
            &checker.class_structs,
            &checker.aliases,
            &checker.generic_alias_decls,
        ),
        Err(e) => return Some(Err(e)),
    };
    // §13.10.1 step 5 makes a non-Object rhs a RUNTIME TypeError, not
    // a compile reject — `"a" in 42` must throw, not fail to build,
    // which is exactly the posture [`try_match_priv`] below has
    // always had. The whitelist that used to sit here (Array /
    // Struct / Function / any) rejected `"get" in new Map()`, a
    // program bun runs, while the identical receiver through an `any`
    // binding answered; the resolve above is kept because `in` asks a
    // STRUCTURAL question and a class instance answers as its struct.
    let _ = obj_ty;
    Some(Ok(Type::Boolean))
}

/// ES2022 §13.10 ergonomic brand check —
/// `__torajs_priv_in_op(key, obj)`, the parser's synthetic for
/// `#x in o` (rotation 297). The key is a compiler-minted mangled
/// String literal (never user data); the rhs may be anything — a
/// non-Object rhs is the runtime kernel's §13.10.1 step-5 TypeError,
/// not a compile reject (`#x in 42` must throw, not fail to build).
pub(crate) fn try_match_priv(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Ident(n) = ast.get_expr(*callee) else {
        return None;
    };
    if n != "__torajs_priv_in_op" || args.len() != 2 {
        return None;
    }
    if let Err(e) = checker.type_of(ast, args[1]) {
        return Some(Err(e));
    }
    Some(Ok(Type::Boolean))
}
