//! Step 1 of the parent module — `Function.prototype.bind.call(f,
//! args…)` becomes `f.bind(args…)` — split from it under the file-size
//! limit.

use crate::ast::{Ast, Expr, Stmt};

/// Step 1 — rewrite `Function.prototype.bind.call(f, args…)` to
/// `f.bind(args…)`. Zero-arg spellings (no receiver to route) stay
/// untouched.
pub fn normalize_function_bind_call(ast: &mut Ast) {
    let user_shadows = ast.stmts.iter().any(|s| {
        matches!(s, Stmt::ClassDecl { name, .. } if name == "Function")
            || matches!(s, Stmt::FnDecl { name, .. } if name == "Function")
            || matches!(s, Stmt::LetDecl { name, .. } if name == "Function")
    });
    if user_shadows {
        return;
    }
    let n = ast.exprs.len();
    for i in 0..n {
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        let (callee, args) = (*callee, args.clone());
        let Expr::Member { obj: m1, name: n1 } = ast.get_expr(callee) else {
            continue;
        };
        if n1 != "call" {
            continue;
        }
        let Expr::Member { obj: m2, name: n2 } = ast.get_expr(*m1) else {
            continue;
        };
        if n2 != "bind" {
            continue;
        }
        let Expr::Member { obj: m3, name: n3 } = ast.get_expr(*m2) else {
            continue;
        };
        if n3 != "prototype" || !matches!(ast.get_expr(*m3), Expr::Ident(n) if n == "Function") {
            continue;
        }
        let Some(&recv) = args.first() else {
            continue;
        };
        // Rotation 431 — a literal wrong-brand receiver (a number /
        // string / bool / null literal or the `undefined` name) keeps
        // the ORIGINAL spelling: §20.2.3.2 step 2 makes it a runtime
        // TypeError (IsCallable(Target) false, t262 this-not-callable
        // probes the throw), while `undefined.bind()` is a
        // compile-time member reject. The un-normalized form rides
        // the reified proto method cell's `.call` dispatch, whose
        // IsCallable gate reads back bun-equal.
        let wrong_brand_literal = matches!(
            ast.get_expr(recv),
            Expr::Number(_) | Expr::Bool(_) | Expr::String(_) | Expr::Null
        ) || matches!(ast.get_expr(recv), Expr::Ident(n) if n == "undefined");
        if wrong_brand_literal {
            continue;
        }
        let bind_callee = ast.add_expr(Expr::Member {
            obj: recv,
            name: "bind".into(),
        });
        ast.exprs[i] = Expr::Call {
            callee: bind_callee,
            args: args[1..].to_vec(),
        };
    }
}
