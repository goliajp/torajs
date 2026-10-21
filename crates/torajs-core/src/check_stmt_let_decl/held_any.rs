//! Bindings of an object type that hold an `any` value.
//!
//! `const s: Shape = raw` with `raw: any` converts nothing: `s` is
//! whatever `raw` holds — a dynamic object, a literal of another
//! layout, a class instance with no witness row for `Shape` — so the
//! lowering keeps the box (`any_layout_slot::value_repr`) and reads its
//! members by name. The binding is still a `Shape` to the checker: a
//! member read or a method call through it types as `Shape` declares
//! it, so `const n: string = s.area()` is refused as tsc refuses it.
//!
//! Only a read that hands the whole value on — an argument, an
//! element, an initializer, a return — types as `any`, as the box it
//! is: those positions then take the lanes an `any` value takes into a
//! typed slot, the same as `raw` itself would, and none of them reads
//! the box at `Shape`'s own offsets. Such a read still has the declared
//! type for the TypeScript rules (`face`), so `const t: string = s` is
//! refused as tsc refuses it.

pub(crate) mod face;
#[cfg(test)]
mod face_tests;

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId};
use crate::check::Type;

#[derive(Debug, Default)]
pub(crate) struct HeldAny {
    /// the top-level bindings that hold one, reached as globals from
    /// function bodies
    pub(crate) globals: HashSet<String>,
    /// the top-level `number` / `string` / `boolean` bindings that hold
    /// one (`super::any_scalar_slot`), by their declared type
    pub(crate) scalar_globals: HashMap<String, Type>,
    /// whether any `number` / `string` / `boolean` binding holds one
    pub(crate) scalars: bool,
    /// the declared type of every read of a binding that holds one and
    /// is typed `any` for the lowering
    pub(crate) reads: HashMap<ExprId, Type>,
    /// every expression read as the receiver of a member access
    receivers: Option<HashSet<ExprId>>,
}

impl HeldAny {
    /// Whether the read `eid` is the receiver of a member access.
    pub(crate) fn is_receiver(&mut self, ast: &Ast, eid: ExprId) -> bool {
        self.receivers
            .get_or_insert_with(|| receivers(ast))
            .contains(&eid)
    }
}

fn receivers(ast: &Ast) -> HashSet<ExprId> {
    (ast.exprs.iter())
        .filter_map(|e| match e {
            Expr::Member { obj, .. }
            | Expr::Index { obj, .. }
            | Expr::OptChain { obj, .. }
            | Expr::OptIndex { obj, .. } => Some(*obj),
            _ => None,
        })
        .collect()
}
