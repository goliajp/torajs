//! `this` inside a body RFC 20260805 moves into a generator — who owns
//! it, and how it becomes the generator's receiver parameter.
//!
//! A moved body's `this` cannot stay `Expr::This`: after the move the
//! body runs inside the `__Gen_*` machine's `next()`, whose `this` is
//! the machine (`parser/parse_class_decl_generator.rs` says why the
//! class generator-method path hands the receiver in as the leading
//! parameter `__genrecv` instead). So the caller of [`rewrite_this`]
//! turns every `this` the body OWNS into a read of that parameter, and
//! its wrapper passes the receiver in — read by the wrapper's own
//! `this`, at the place the user's function stood, so `desugar_classes`
//! / `bind_this_param` / `rewrite_toplevel_this` bind it exactly as
//! they would have bound the unmoved body's.
//!
//! An arrow's `this` is lexical (§8.3.4) and IS the body's. A function
//! EXPRESSION and an object-literal method own theirs (the parser
//! parks all three in `Expr::ArrowFn`, telling them apart through
//! `fn_expr_exprs` / `objlit_method_exprs` — the same split
//! `desugar_generators_rewrite` uses), so both the move gate and the
//! rewrite stop at them: an async method whose body held `function ()
//! { return this.z }` used to have THAT `this` rewritten to the
//! method's receiver (`inner.call({ z: 1 })` answered the outer
//! object's field).

use super::desugar_with::walk;
use super::{Ast, Expr, ExprId, Stmt};

/// An `Expr::ArrowFn` that binds its own `this` at the call site: a
/// function expression or an object-literal method.
pub(super) fn owns_this(ast: &Ast, eid: ExprId) -> bool {
    ast.fn_expr_exprs.contains(&eid) || ast.objlit_method_exprs.contains(&eid)
}

/// `this` → `Ident(recv)` across the body's own `this` reads: through
/// arrows, not into a nested `FnDecl` (`stmt_children_ref` stops at
/// it) nor into an expression that binds its own ([`owns_this`]).
pub(super) fn rewrite_this(ast: &mut Ast, body: &[Stmt], recv: &str) {
    for s in body {
        stmt_this_to_recv(ast, s, recv);
    }
}

fn stmt_this_to_recv(ast: &mut Ast, s: &Stmt, recv: &str) {
    for e in walk::stmt_exprs(s) {
        expr_this_to_recv(ast, e, recv);
    }
    for c in walk::stmt_children_ref(s) {
        stmt_this_to_recv(ast, c, recv);
    }
}

fn expr_this_to_recv(ast: &mut Ast, eid: ExprId, recv: &str) {
    match ast.get_expr(eid) {
        Expr::This => {
            ast.exprs[eid.0 as usize] = Expr::Ident(recv.into());
        }
        Expr::ArrowFn { .. } if owns_this(ast, eid) => {}
        Expr::ArrowFn { body, .. } => {
            let body = body.clone();
            for s in &body {
                stmt_this_to_recv(ast, s, recv);
            }
        }
        _ => {
            for c in walk::expr_children(ast, eid) {
                expr_this_to_recv(ast, c, recv);
            }
        }
    }
}
