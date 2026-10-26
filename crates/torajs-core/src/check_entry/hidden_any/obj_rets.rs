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

/// The functions of a program declared to return an object type
/// (`super::retypable::object_ann`), with the values they return: what
/// every check round asks again with its own types.
pub(super) struct ObjRets(Vec<(String, Vec<ExprId>)>);

impl ObjRets {
    pub(super) fn of(ast: &Ast) -> Self {
        let mut out = Vec::new();
        crate::ast::walk_stmts(&ast.stmts, &mut |s| {
            if let Stmt::FnDecl {
                name,
                return_type: Some(rt),
                body,
                ..
            } = s
                && super::retypable::object_ann(rt)
            {
                out.push((name.clone(), own_returns(body)));
            }
        });
        ObjRets(out)
    }

    /// Those with a `return` of a value `types` types `any`.
    pub(super) fn found(&self, types: &HashMap<ExprId, Type>) -> HashSet<String> {
        (self.0.iter())
            .filter(|(_, rets)| rets.iter().any(|e| types.get(e) == Some(&Type::Any)))
            .map(|(name, _)| name.clone())
            .collect()
    }
}
