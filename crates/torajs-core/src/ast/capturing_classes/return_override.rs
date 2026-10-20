//! §10.2.2 [[Construct]] step 13 on the runtime-value lane: what a
//! derived constructor's `this` IS once `super(…)` has answered.
//!
//! The lane's constructor is a function expression run on an object
//! `new` allocated, and its `super(…)` used to drop the parent's
//! answer — `class K extends function (x) { return x } {}` built a
//! fresh object where the spec (and every field initializer, private
//! ones included) wants the one the parent handed back. The shape is
//! the real-class lane's (`desugar_classes_ctor_return`), with the
//! same kernel doing the pick:
//!
//! ```text
//! function (…) {
//!   let __sthis: any = this;          // every `this` below reads it
//!   let __sup: any = undefined;
//!   ( __sup = <parent call>,
//!     __sthis = __torajs_ctor_ret_value(__sthis, __sup, false),
//!     __sthis );                      // the rewritten `super(…)`
//!   …
//!   return __sthis;
//! }
//! ```
//!
//! `extends::adopt_super_answer` writes the sequence with `this`
//! nodes; [`reshape`] then turns every `this` of the constructor's own
//! function into the local — arrows ride along (lexical `this`), a
//! nested `function` / class keeps its own. All three super paths
//! answer a value: a runtime-value parent, another class of this lane
//! through `.call`, and a real class through its `__ctorany_` twin.
//!
//! Unlike the real lane, the `new` that runs this function picks
//! against the object IT allocated, so the returns are rewritten too:
//! a bare `return;` answers the local, and `return <expr>` takes step
//! 13 here with the class as the derived one it is (13.c: a
//! non-object other than undefined is a TypeError). The candidate
//! lands in `__sup` first for the reason `reshape_ctor` records — a
//! call result fed straight to another call is never released.

use super::super::desugar_with::walk::{
    expr_children, stmt_children, stmt_children_ref, stmt_exprs,
};
use super::super::{Ast, Expr, ExprId, Stmt};

/// The constructor-local `this`.
pub(super) const STHIS: &str = "__sthis";
/// What the last `super(…)` (or a written `return <expr>`) answered.
pub(super) const SUP: &str = "__sup";

pub(super) fn reshape(ast: &mut Ast, body: &mut Vec<Stmt>) {
    let mut this_nodes = Vec::new();
    let mut adopts = false;
    for s in body.iter() {
        scan_stmt(ast, s, &mut this_nodes, &mut adopts);
    }
    if !adopts {
        return;
    }
    for eid in this_nodes {
        ast.exprs[eid.0 as usize] = Expr::Ident(STHIS.to_string());
    }
    for s in body.iter_mut() {
        rewrite_returns(ast, s);
    }
    let this = ast.add_expr(Expr::This);
    let undef = ast.add_expr(Expr::Ident("undefined".to_string()));
    let local = |name: &str, init| Stmt::LetDecl {
        mutable: true,
        name: name.to_string(),
        type_ann: Some("any".to_string()),
        init,
        is_var: false,
    };
    body.splice(0..0, [local(STHIS, this), local(SUP, undef)]);
    let tail = ast.add_expr(Expr::Ident(STHIS.to_string()));
    body.push(Stmt::Return(Some(tail)));
}

/// A nested function or class binds its own `this` and its own
/// returns.
fn own_scope(s: &Stmt) -> bool {
    matches!(s, Stmt::FnDecl { .. } | Stmt::ClassDecl { .. })
}

fn scan_stmt(ast: &Ast, s: &Stmt, this_nodes: &mut Vec<ExprId>, adopts: &mut bool) {
    if own_scope(s) {
        return;
    }
    for e in stmt_exprs(s) {
        scan_expr(ast, e, this_nodes, adopts);
    }
    for c in stmt_children_ref(s) {
        scan_stmt(ast, c, this_nodes, adopts);
    }
}

fn scan_expr(ast: &Ast, root: ExprId, this_nodes: &mut Vec<ExprId>, adopts: &mut bool) {
    let mut pending = vec![root];
    while let Some(eid) = pending.pop() {
        match ast.get_expr(eid) {
            Expr::This => this_nodes.push(eid),
            Expr::Ident(n) if n == SUP => *adopts = true,
            Expr::ArrowFn { body, .. } => {
                if !ast.fn_expr_exprs.contains(&eid) {
                    for s in body {
                        scan_stmt(ast, s, this_nodes, adopts);
                    }
                }
                continue;
            }
            _ => {}
        }
        pending.extend(expr_children(ast, eid));
    }
}

fn rewrite_returns(ast: &mut Ast, s: &mut Stmt) {
    if own_scope(s) {
        return;
    }
    if let Stmt::Return(answer) = s {
        let picked = match *answer {
            None => ast.add_expr(Expr::Ident(STHIS.to_string())),
            Some(e) => {
                let slot = ast.add_expr(Expr::Ident(SUP.to_string()));
                let store = ast.add_expr(Expr::Assign {
                    target: slot,
                    value: e,
                });
                let incumbent = ast.add_expr(Expr::Ident(STHIS.to_string()));
                let candidate = ast.add_expr(Expr::Ident(SUP.to_string()));
                let pick = super::super::desugar_classes_ctor_return::pick_call(
                    ast, incumbent, candidate, true,
                );
                ast.add_expr(Expr::Sequence {
                    left: store,
                    right: pick,
                })
            }
        };
        *answer = Some(picked);
        return;
    }
    for c in stmt_children(s) {
        rewrite_returns(ast, c);
    }
}
