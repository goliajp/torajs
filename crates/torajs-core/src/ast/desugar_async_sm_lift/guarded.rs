//! The statement form the lift gives a guarded node (`&&`, `||`, `??`,
//! a ternary) whose guarded side awaits.

use super::{LiftLog, replace};
use crate::ast::{Ast, BinOp, Expr, ExprId, Stmt, UnaryOp};

/// The statement form of a guarded node whose guarded side awaits
/// (module doc): `let __awg: any = undefined; if (…) { __awg = … } else
/// { __awg = … }` ahead of the statement, the node reading the temp.
/// The branches still hold their awaits — the caller's rewrite of the
/// prefix turns them into yields.
pub(super) fn guarded_to_statements(
    ast: &mut Ast,
    eid: ExprId,
    prefix: &mut Vec<Stmt>,
    log: &mut LiftLog,
) {
    let tmp = format!("__awg_{}", eid.0);
    let assign_tmp = |ast: &mut Ast, value: ExprId| -> Stmt {
        let target = ast.add_expr(Expr::Ident(tmp.clone()));
        Stmt::Expr(ast.add_expr(Expr::Assign { target, value }))
    };
    let (init, cond, then_v, else_v): (Option<ExprId>, ExprId, ExprId, Option<ExprId>) =
        match ast.get_expr(eid).clone() {
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => (None, cond, then_branch, Some(else_branch)),
            // `l && r`: g = l; if (g) g = r.   `l || r`: if (!g) g = r.
            // `l ?? r`: if (g == null) g = r.
            Expr::BinOp {
                op: BinOp::LAnd,
                left,
                right,
            } => (
                Some(left),
                ast.add_expr(Expr::Ident(tmp.clone())),
                right,
                None,
            ),
            Expr::BinOp {
                op: BinOp::LOr,
                left,
                right,
            } => {
                let g = ast.add_expr(Expr::Ident(tmp.clone()));
                let not = ast.add_expr(Expr::Unary {
                    op: UnaryOp::Not,
                    expr: g,
                });
                (Some(left), not, right, None)
            }
            Expr::Nullish { lhs, rhs } => {
                let g = ast.add_expr(Expr::Ident(tmp.clone()));
                let null = ast.add_expr(Expr::Null);
                let is_nullish = ast.add_expr(Expr::BinOp {
                    op: BinOp::LooseEq,
                    left: g,
                    right: null,
                });
                (Some(lhs), is_nullish, rhs, None)
            }
            other => panic!("guarded lift on a non-guarded node: {other:?}"),
        };
    let init = init.unwrap_or_else(|| ast.add_expr(Expr::Ident("undefined".into())));
    prefix.push(Stmt::LetDecl {
        mutable: true,
        name: tmp.clone(),
        type_ann: Some("any".into()),
        init,
        is_var: false,
    });
    let then_s = assign_tmp(ast, then_v);
    let else_s = else_v.map(|v| Box::new(assign_tmp(ast, v)));
    prefix.push(Stmt::If {
        cond,
        then_branch: Box::new(then_s),
        else_branch: else_s,
    });
    replace(ast, eid, Expr::Ident(tmp), log);
}
