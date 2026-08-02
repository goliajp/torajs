//! Zero-argument `Function()` / `new Function()` — §20.2.1.1 with an
//! empty *bodyText*: CreateDynamicFunction over no source is exactly
//! an anonymous empty function, so the call desugars to the arrow
//! literal `() => {}` (the parser's uniform function-expression
//! shape) and rides the ordinary closure pipeline. No dynamic code
//! is involved — the argument-bearing forms (`Function("return x")`)
//! ARE dynamic code and stay on today's loud `not callable` reject
//! until the eval-family RFC frames them (t262 census r283: 9 of the
//! 136-case cluster are the zero-arg shape).

use crate::ast::{Ast, Expr};

/// Rewrite every zero-arg `Function()` call and `new Function()` to
/// an empty `ArrowFn`. A user binding named `Function` owns the name
/// (same shadow rule as the sibling `Promise` pass).
pub(super) fn rewrite_function_zero_arg(ast: &mut Ast) {
    let user_shadows = ast.stmts.iter().any(|s| {
        matches!(s, crate::ast::Stmt::ClassDecl { name, .. } if name == "Function")
            || matches!(s, crate::ast::Stmt::FnDecl { name, .. } if name == "Function")
            || matches!(s, crate::ast::Stmt::LetDecl { name, .. } if name == "Function")
    });
    if user_shadows {
        return;
    }
    let n = ast.exprs.len();
    for i in 0..n {
        let hit = match &ast.exprs[i] {
            Expr::Call { callee, args } if args.is_empty() => {
                matches!(ast.get_expr(*callee), Expr::Ident(name) if name == "Function")
            }
            Expr::New {
                class_name, args, ..
            } => class_name == "Function" && args.is_empty(),
            _ => false,
        };
        if hit {
            ast.exprs[i] = Expr::ArrowFn {
                params: Vec::new(),
                return_type: None,
                body: Vec::new(),
            };
        }
    }
}
