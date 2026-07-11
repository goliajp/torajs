//! Annex B B.2.2 `s.{anchor,fontcolor,fontsize,link,big,blink,bold,
//! fixed,italics,small,strike,sub,sup}(...)` HTML wrap methods.
//!
//! One arm claims every arity: B.2.2.2.1 CreateHTML runs ToString on
//! the attribute value unconditionally, so the four attributed forms
//! accept ANY first argument (`"x".fontsize(7)` is legal, a missing
//! value renders "undefined"), and the spec reserves no further
//! positional slots — every remaining argument is typechecked for
//! side effects and ignored (S272 idiom; ssa_lower mirror lowers and
//! drops). Always returns `Type::String` on a String receiver.
//!
//! Returns `None` on a non-String receiver or a non-HTML method
//! name (cascade falls through).

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
        "anchor"
            | "fontcolor"
            | "fontsize"
            | "link"
            | "big"
            | "blink"
            | "bold"
            | "fixed"
            | "italics"
            | "small"
            | "strike"
            | "sub"
            | "sup"
    ) {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    for &a in args {
        if let Err(e) = checker.type_of(ast, a) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::String))
}
