//! Functions declared to return an object type that return an `any`
//! value.
//!
//! `function f(): Shape { return raw }` with `raw: any` returns what
//! `raw` holds (§10.2.1.12): a dynamic object, an instance of another
//! class, a number. A return slot typed by the annotation reads the
//! value at the layout's offsets, so the value would have to be copied
//! into the layout or rejected, and neither is what the program says.
//! The return is retyped `any` as a scalar one is
//! (`super::retype_returns_any`); the round that checks the program as
//! written still holds the callers to the declared type. Unlike a
//! scalar return, whatever the `any` value traces back to does not
//! matter: a value of the right layout is the only one the slot could
//! read, and nothing at compile time says that it is one.

use std::collections::{HashMap, HashSet};

use super::promise_rets::own_returns;
use crate::ast::{Ast, ExprId, Stmt};
use crate::check::Type;

/// The functions of `ast` declared to return an object type
/// (`super::retypable::retypable_ann`) with a `return` of a value
/// `types` types `any`.
pub(super) fn found(ast: &Ast, types: &HashMap<ExprId, Type>) -> HashSet<String> {
    let mut out = HashSet::new();
    crate::ast::walk_stmts(&ast.stmts, &mut |s| {
        let Stmt::FnDecl {
            name,
            return_type: Some(rt),
            body,
            ..
        } = s
        else {
            return;
        };
        if super::retypable::object_ann(rt)
            && (own_returns(body).iter()).any(|e| types.get(e) == Some(&Type::Any))
        {
            out.insert(name.clone());
        }
    });
    out
}
