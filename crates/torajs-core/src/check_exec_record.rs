//! The checker's side of the exec-record lane: which `let` bindings
//! hold a `re.exec(s)` result as spans, and what `m.index` / `m.input`
//! read on them.
//!
//! §22.2.7.8 gives every exec hit an `index` (a number) and an `input`
//! (the subject string). On an ordinary `string[] | null` those names
//! read as `any`: the array's expando bag is writable and reachable
//! through every alias, so the checker cannot promise what sits under
//! them. A record binding is different — the census
//! (`ast::regex_exec_record`) admits it only when nothing but this
//! lane's own reads and the same regex's re-assignments ever touch it,
//! so the two names are exactly what the kernel wrote: `index` is a
//! number and `input` a string, and the arithmetic around them stays
//! on the number lane instead of boxing.
//!
//! The binding's type is untouched — `string[] | null` as before, so
//! every use the census admits types as it always did — and the
//! lowering takes the lane on the same three facts asked here: the
//! census listed the declaration's call, the binding is unannotated,
//! and the receiver is a RegExp.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

/// `init` is a `re.exec(s)` the census listed, on a RegExp receiver,
/// declaring an unannotated binding.
pub(crate) fn init_is_record(
    checker: &mut Checker,
    ast: &Ast,
    type_ann: &Option<String>,
    init: ExprId,
) -> bool {
    if type_ann.is_some() {
        return false;
    }
    let Expr::Call { callee, .. } = ast.get_expr(init) else {
        return false;
    };
    if !ast.regex_exec_record_callees.contains_key(callee) {
        return false;
    }
    let Expr::Member { obj, .. } = ast.get_expr(*callee) else {
        return false;
    };
    checker.type_of(ast, *obj) == Ok(Type::RegExp)
}

/// `obj.name` when `obj` names a record binding and `name` is one of
/// the two exec-shape properties the record holds: `index` is a
/// number, `input` a string. `obj_ty` is the receiver's type after the
/// nullable decay, so an un-narrowed `m.index` answers too (the
/// lowering guards the miss the way it guards `m[0]`).
pub(crate) fn member_type(
    checker: &Checker,
    ast: &Ast,
    obj: ExprId,
    obj_ty: &Type,
    name: &str,
) -> Option<Type> {
    if !matches!(obj_ty, Type::Array(_)) {
        return None;
    }
    let Expr::Ident(n) = ast.get_expr(obj) else {
        return None;
    };
    if !checker.lookup(n).is_some_and(|info| info.exec_result) {
        return None;
    }
    match name {
        "index" => Some(Type::Number),
        "input" => Some(Type::String),
        _ => None,
    }
}
