//! The rewrite half of the closure TDZ flag — what each collected
//! [`Site`] becomes — split out of the parent under the file-size line.

use super::Site;
use crate::ast::clone_body::BodyCloner;
use crate::ast::dflt_param_tdz::tdz_throw_iife;
use crate::ast::{Ast, Expr, ExprId};

/// `flag ? undefined : <raise>` — the guard every site's rewrite
/// hangs off; its own value is never used.
fn guard(ast: &mut Ast, name: &str, flag: &str) -> ExprId {
    let cond = ast.add_expr(Expr::Ident(flag.to_string()));
    let then_branch = ast.add_expr(Expr::Ident("undefined".to_string()));
    let raise = tdz_throw_iife(ast, name, None);
    let else_branch = ast.add_expr(raise);
    ast.add_expr(Expr::Ternary {
        cond,
        then_branch,
        else_branch,
    })
}

pub(in crate::ast) fn rewrite_site(ast: &mut Ast, site: Site, name: &str, flag: &str) {
    match site {
        Site::Read(eid) => {
            let left = guard(ast, name, flag);
            let right = ast.add_expr(Expr::Ident(name.to_string()));
            ast.exprs[eid.0 as usize] = Expr::Sequence { left, right };
        }
        Site::Write { assign, value } => {
            // `flag ? (x = v) : (v, raise)` — the right-hand side is
            // evaluated on both arms (§13.15.2 step 1.f precedes the
            // PutValue that throws), the dead arm on its own copy.
            let cond = ast.add_expr(Expr::Ident(flag.to_string()));
            let target = ast.add_expr(Expr::Ident(name.to_string()));
            let then_branch = ast.add_expr(Expr::Assign { target, value });
            let copy = {
                let mut cloner = BodyCloner::new(ast);
                let copy = cloner.clone_expr(value);
                crate::ast::clone_body_tables::migrate(&mut cloner);
                copy
            };
            let raise = tdz_throw_iife(ast, name, Some(copy));
            let else_branch = ast.add_expr(raise);
            ast.exprs[assign.0 as usize] = Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            };
        }
        Site::Incr {
            node,
            is_inc,
            is_prefix,
        } => {
            let cond = ast.add_expr(Expr::Ident(flag.to_string()));
            let target = ast.add_expr(Expr::Ident(name.to_string()));
            let then_branch = ast.add_expr(Expr::PostIncr {
                target,
                is_inc,
                is_prefix,
            });
            let raise = tdz_throw_iife(ast, name, None);
            let else_branch = ast.add_expr(raise);
            ast.exprs[node.0 as usize] = Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            };
        }
    }
}
