//! The face-slot census of the object literal arguments of a call whose
//! argument list keeps a dynamic `...spread`.
//!
//! Such a call types as `any` and runs through the runtime spread
//! kernels, so no parameter is paired with an argument at compile
//! time. A literal before the first spread still lands on the
//! parameter at its position; one after it lands on its position or
//! any later one, as many as the spread turns out to supply
//! (§13.3.8.1). Each parameter it may land on records its member
//! stores, as a paired argument's does: a member slot recorded for a
//! store that does not happen only takes the boxed entry for nothing.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

/// Record the member stores of the object literals among `args` into
/// the parameters of `callee` they may land on.
pub(super) fn record(checker: &mut Checker, ast: &Ast, callee: ExprId, args: &[ExprId]) {
    let Expr::Ident(name) = ast.get_expr(callee) else {
        return;
    };
    let Some(Type::Function(params, _)) = checker.globals.get(name).cloned() else {
        return;
    };
    let (mut at, mut spread) = (0usize, false);
    for &a in args {
        match ast.get_expr(a) {
            Expr::Spread { .. } => spread = true,
            Expr::ObjectLit { .. } => {
                let last = if spread { params.len() } else { at + 1 };
                for p in params.iter().take(last).skip(at) {
                    let slot = match p {
                        Type::Rest(elem) => elem.as_ref(),
                        p => p,
                    };
                    crate::check::face_slots::record_objlit_stores(checker, ast, slot, a);
                }
                if let Some(Type::Rest(elem)) = params.last()
                    && at >= params.len()
                {
                    crate::check::face_slots::record_objlit_stores(checker, ast, elem, a);
                }
                at += 1;
            }
            _ => at += 1,
        }
    }
}
