//! Knife 2 of `new_target_param`: the function expressions that
//! read `new.target` (split out of the parent).

use super::{NEW_TARGET, collect_stmts};
use crate::ast::{Ast, Expr, ExprId, Stmt};

/// Knife 2 — a function EXPRESSION reading `new.target`. Its closure
/// value is the function itself, so no head can change: every call
/// through the value would shift. The reads become a body-level
/// `let __new_target = undefined` (an arrow inside captures it like any
/// local), which is the [[Call]] answer; the checker's monomorph step
/// later mints a construct face from the lifted body with the slot as a
/// parameter (`check_monomorph_ctor_face.rs`), and the construct kernel
/// enters that face with the new.target. Recorded by ExprId, which
/// `lift_arrow_fns` keeps (it rewrites the arena slot in place).
pub(super) fn bind_fn_expr_new_target(ast: &mut Ast) {
    let mut exprs: Vec<ExprId> = ast.fn_expr_exprs.iter().copied().collect();
    exprs.sort_by_key(|e| e.0);
    for eid in exprs {
        let Expr::ArrowFn { body, .. } = ast.get_expr(eid) else {
            continue;
        };
        let mut reads = Vec::new();
        collect_stmts(ast, body, &mut reads);
        if reads.is_empty() {
            continue;
        }
        for r in reads {
            ast.exprs[r.0 as usize] = Expr::Ident(NEW_TARGET.into());
        }
        let undef = ast.add_expr(Expr::Ident("undefined".into()));
        if let Expr::ArrowFn { body, .. } = &mut ast.exprs[eid.0 as usize] {
            body.insert(
                0,
                Stmt::LetDecl {
                    mutable: false,
                    name: NEW_TARGET.into(),
                    type_ann: Some("any".into()),
                    init: undef,
                    is_var: false,
                },
            );
        }
        ast.new_target_fn_exprs.insert(eid);
    }
}
