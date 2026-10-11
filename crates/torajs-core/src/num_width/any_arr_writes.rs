//! Whether a function body may write through its `any[]` parameter.
//!
//! A typed array handed to a slot the program annotated `any[]` takes
//! values of any kind only when something writes through that slot:
//! a mutating method (`push`, `splice`, `fill`, …), an element or
//! `length` store, or the array going on to some other place that may
//! do either. A body that only reads the parameter (`xs[i]`,
//! `xs.length`, `for (const v of xs)`, a non-mutating method whose
//! callback cannot see the array) leaves the typed block as it is.
//!
//! The answer is conservative: any occurrence of the name outside
//! those read shapes counts as a write, and so does a nested function
//! or a closure that captures it.

use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Stmt};

/// Whether `body` may write through the array its parameter `name`
/// holds.
pub(super) fn param_written(ast: &Ast, body: &[Stmt], name: &str) -> bool {
    let mut nested = false;
    crate::ast::walk_stmts(body, &mut |s| {
        nested |= matches!(s, Stmt::FnDecl { .. } | Stmt::ClassDecl { .. });
    });
    if nested {
        return true;
    }
    let mut written = false;
    for s in body {
        visit_stmt_exprs(s, &mut |e| written |= expr_writes(ast, e, name));
    }
    written
}

fn expr_writes(ast: &Ast, e: ExprId, name: &str) -> bool {
    let is_name = |x: ExprId| matches!(ast.get_expr(x), Expr::Ident(n) if n == name);
    match ast.get_expr(e) {
        Expr::Ident(n) => n == name,
        Expr::Assign { target: t, .. }
        | Expr::PostIncr { target: t, .. }
        | Expr::Delete { expr: t }
            if store_through(ast, *t, name) =>
        {
            true
        }
        Expr::Closure { captures, .. } => captures.iter().any(|c| c == name),
        Expr::ArrowFn { .. } => true,
        Expr::Index { obj, index } if is_name(*obj) => expr_writes(ast, *index, name),
        Expr::Member { obj, name: m } if is_name(*obj) => m != "length",
        Expr::Call { callee, args } => match ast.get_expr(*callee) {
            Expr::Member { obj, name: m } if is_name(*obj) && reads_only(ast, m, args) => {
                args.iter().any(|a| expr_writes(ast, *a, name))
            }
            _ => children_write(ast, e, name),
        },
        _ => children_write(ast, e, name),
    }
}

/// A store target rooted at `name` itself: the binding, one of its
/// elements or one of its properties (`length` among them).
fn store_through(ast: &Ast, t: ExprId, name: &str) -> bool {
    match ast.get_expr(t) {
        Expr::Ident(n) => n == name,
        Expr::Index { obj, .. } | Expr::Member { obj, .. } => {
            matches!(ast.get_expr(*obj), Expr::Ident(n) if n == name)
        }
        _ => false,
    }
}

fn children_write(ast: &Ast, e: ExprId, name: &str) -> bool {
    let mut written = false;
    visit_expr_children(ast, e, &mut |c| written |= expr_writes(ast, c, name));
    written
}

/// A method of `Array.prototype` that neither writes its receiver nor
/// hands it on: one taking no callback, or one whose callback is a
/// closure declaring fewer parameters than the position the method
/// passes the array at (§23.1.3: `(value, index, array)`, and
/// `(acc, value, index, array)` for the reducers).
fn reads_only(ast: &Ast, method: &str, args: &[ExprId]) -> bool {
    let array_at = match method {
        "at" | "includes" | "indexOf" | "lastIndexOf" | "join" | "slice" | "concat" | "entries"
        | "keys" | "values" | "toString" | "toLocaleString" => return true,
        "every" | "some" | "filter" | "find" | "findIndex" | "findLast" | "findLastIndex"
        | "forEach" | "map" | "flatMap" => 2,
        "reduce" | "reduceRight" => 3,
        _ => return false,
    };
    args.first().is_some_and(|cb| match ast.get_expr(*cb) {
        Expr::Closure { fn_name, .. } => ast.stmts.iter().any(|s| {
            matches!(s, Stmt::FnDecl { name, params, .. } if name == fn_name
                && params.iter().filter(|p| !matches!(p.name.as_str(), "__env" | "__this")).count()
                    <= array_at)
        }),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::param_written;

    fn written(src: &str) -> bool {
        let tokens = crate::lexer::tokenize(src).expect("lex");
        let mut ast = crate::parser::parse(src, &tokens).expect("parse");
        crate::ast::lift_arrow_fns(&mut ast);
        let body = ast.stmts.iter().find_map(|s| match s {
            crate::ast::Stmt::FnDecl { name, body, .. } if name == "f" => Some(body.clone()),
            _ => None,
        });
        param_written(&ast, &body.expect("f"), "xs")
    }

    #[test]
    fn reads_leave_the_parameter_unwritten() {
        assert!(!written(
            "function f(xs: any[]) { let s = 0; for (const v of xs) s += v; return s; }"
        ));
        assert!(!written(
            "function f(xs: any[]) { let s = 0; for (let i = 0; i < xs.length; i++) s += xs[i]; \
             return s; }"
        ));
        assert!(!written(
            "function f(xs: any[]) { return xs.map((v) => v + 1).join(\",\") + xs.indexOf(1); }"
        ));
        assert!(!written(
            "function f(xs: any[]) { return xs.reduce((a, v, i) => a + v * i, 0); }"
        ));
    }

    #[test]
    fn writes_and_hand_ons_count() {
        for body in [
            "xs.push(\"s\");",
            "xs[0] = \"s\";",
            "xs.length = 0;",
            "xs.splice(0, 1);",
            "g(xs);",
            "const ys = xs; ys.push(1);",
            "return xs;",
            "xs.forEach((v, i, a) => a.push(v));",
            "xs.reduce((acc, v, i, a) => a, 0);",
            "const h = () => xs.push(1); h();",
        ] {
            let src = format!("function g(a: any) {{}}\nfunction f(xs: any[]) {{ {body} }}");
            assert!(written(&src), "{body}");
        }
    }
}
