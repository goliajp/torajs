//! `xs.push(v)` / `xs.unshift(v)` with `v: any`, on a binding without
//! an annotation whose initializer is an array of an object layout.
//!
//! The element type is the compiler's inference from the initializer,
//! and `v` may hold a value of any kind, which §23.1.3.23 appends as it
//! is. An inferred slot never rejects a value at run time (RFC 20260922
//! §3.1, the third row), so the binding is `any` from its declaration:
//! its elements are read through the dynamic lane, each as what it is.
//! An annotated binding keeps its element type; the lowering casts the
//! value into it (`ssa_lower_checked_cast::obj`).

use super::{Binding, Walker};
use crate::ast::{Expr, ExprId};
use crate::check::Type;

impl Walker<'_> {
    pub(super) fn scan_push(&mut self, callee: ExprId, args: &[ExprId]) {
        let Some(types) = self.types else { return };
        let Expr::Member { obj, name } = self.ast.get_expr(callee) else {
            return;
        };
        let Expr::Ident(recv) = self.ast.get_expr(*obj) else {
            return;
        };
        if !matches!(name.as_str(), "push" | "unshift")
            || !args.iter().any(|a| types.get(a) == Some(&Type::Any))
        {
            return;
        }
        let init = match self.scopes.iter().rev().find_map(|s| s.get(recv)) {
            Some(Binding::Tracked { init, .. } | Binding::Fixed { init }) => *init,
            _ => return,
        };
        if matches!(types.get(&init), Some(Type::Array(el))
            if matches!(**el, Type::Struct(_) | Type::ClassRef(_)))
        {
            self.out.insert(init);
        }
    }
}
