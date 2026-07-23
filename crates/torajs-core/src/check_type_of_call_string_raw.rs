//! `String.raw(template, ...substitutions)` per ES §22.1.2.4 (the
//! direct-fn call shape; the tagged-template literal surface
//! `String.raw\`...\`` is a separate parser + emitter substrate
//! item, not covered here).
//!
//! Accepts any template shape (`{raw: string[]}` is the intended
//! contract but the runtime kernel validates it at execution) and
//! any-typed substitutions, answers `Type::String`. A missing
//! template arg yields the same TypeError the runtime would raise;
//! reject at typecheck so the message points at source.
//!
//! Returns `Some(Ok(String))` on match, `Some(Err(_))` on missing
//! args or an arg that fails its own typecheck, `None` when the
//! callee isn't `String.raw`.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member { obj, name: m } = ast.get_expr(*callee) else {
        return None;
    };
    if m != "raw" {
        return None;
    }
    let Expr::Ident(ns) = ast.get_expr(*obj) else {
        return None;
    };
    if ns != "String" {
        return None;
    }
    if args.is_empty() {
        return Some(Err(
            "String.raw requires a template arg (per ES §22.1.2.4)".into()
        ));
    }
    for &aid in args {
        if let Err(e) = checker.type_of(ast, aid) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::String))
}
