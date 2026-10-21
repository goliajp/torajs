//! The order the early-route arms are tried in, and the one answer they
//! all share for `.catch`.

use super::{promise_of, try_then_array, try_then_heterogeneous, try_then_two_arg};
use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};
use crate::check_type_of_call_promise_then_nullable::try_then_nullable;

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &[ExprId],
) -> Option<Result<Type, String>> {
    let r = try_then_two_arg(checker, ast, callee, args)
        .or_else(|| try_then_heterogeneous(checker, ast, callee, args))
        .or_else(|| {
            crate::check_type_of_call_promise_then_undefined::try_then_undefined(
                checker, ast, callee, args,
            )
        })
        .or_else(|| try_then_array(checker, ast, callee, args))
        .or_else(|| try_then_nullable(checker, ast, callee, args))
        .or_else(|| {
            crate::check_type_of_call_promise_then_zero::try_then_zero_arg(
                checker, ast, callee, args,
            )
        });
    match r {
        Some(Ok(Type::Promise(handled))) => {
            Some(catch_passes_source(checker, ast, callee, *handled))
        }
        r => r,
    }
}

/// The arms type a one-handler `.catch(onErr)` by onErr's return, the
/// way they type `.then(onOk)`. §27.2.5.1 makes `.catch(onErr)` a
/// `.then(undefined, onErr)`: a fulfilled source settles the derived
/// promise with its own value, so the value is the source's or onErr's,
/// their union. Typed by onErr alone, an awaited `.catch` of a
/// fulfilled `Promise<number>` read its number as a string pointer.
fn catch_passes_source(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    handled: Type,
) -> Result<Type, String> {
    let Expr::Member { obj, name } = ast.get_expr(*callee) else {
        return Ok(Type::Promise(Box::new(handled)));
    };
    if name != "catch" || is_reject_mint(ast, *obj) {
        return Ok(Type::Promise(Box::new(handled)));
    }
    match crate::check::receiver_type(checker, ast, *obj)? {
        Type::Promise(source) => Ok(promise_of(&super::two_leg_result([*source, handled]))),
        _ => Ok(Type::Promise(Box::new(handled))),
    }
}

/// Is `eid` a `Promise.reject(...)` mint? It never fulfils, so a
/// `.catch` on it settles with the handler's value alone. Its checker
/// type is `Promise<any>` (`any` standing in for `never` at assignment
/// sites), which a join would read as the top type.
pub(crate) fn is_reject_mint(ast: &Ast, eid: ExprId) -> bool {
    let Expr::Call { callee, .. } = ast.get_expr(eid) else {
        return false;
    };
    matches!(
        ast.get_expr(*callee),
        Expr::Member { obj, name } if name == "reject"
            && matches!(ast.get_expr(*obj), Expr::Ident(p) if p == "Promise")
    )
}
