//! `walk_expr` — the expression half of the free-variable walk, the
//! sibling of `free_vars::walk_stmt`.
//!
//! Split out of `free_vars.rs` when that file reached 494 of its 500
//! lines. The seam is the one the module doc there already names:
//! statements bind, expressions read. The two walkers stay mutually
//! recursive across the module boundary — an expression carrying a
//! function body walks statements again.

use super::free_vars::{is_global_name, walk_stmt};
use super::free_vars_hoisted_names::hoist_fn_decl_names;
use super::{Ast, Expr, ExprId};

pub(super) fn walk_expr(ast: &Ast, eid: ExprId, bound: &mut Vec<String>, out: &mut Vec<String>) {
    match ast.get_expr(eid) {
        Expr::Elision => {}
        Expr::Ident(name) => {
            if is_global_name(name) {
                return;
            }
            if !bound.contains(name) && !out.contains(name) {
                out.push(name.clone());
            }
        }
        Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::Regex { .. } => {}
        Expr::BinOp { left, right, .. } => {
            walk_expr(ast, *left, bound, out);
            walk_expr(ast, *right, bound, out);
        }
        Expr::Unary { expr, .. } => walk_expr(ast, *expr, bound, out),
        Expr::Member { obj, .. } => walk_expr(ast, *obj, bound, out),
        Expr::Call { callee, args } => {
            walk_expr(ast, *callee, bound, out);
            for a in args {
                walk_expr(ast, *a, bound, out);
            }
        }
        Expr::Assign { target, value } => {
            walk_expr(ast, *target, bound, out);
            walk_expr(ast, *value, bound, out);
        }
        Expr::Index { obj, index } => {
            walk_expr(ast, *obj, bound, out);
            walk_expr(ast, *index, bound, out);
        }
        Expr::Array(elems) => {
            for e in elems {
                walk_expr(ast, *e, bound, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                walk_expr(ast, *e, bound, out);
            }
        }
        Expr::ArrowFn { params, body, .. } => {
            let saved = bound.len();
            for p in params {
                bound.push(p.name.clone());
            }
            // A marked fn-expr (`function () {}` parsed into an
            // ArrowFn node and listed in `ast.fn_expr_exprs`) and an
            // object-literal method shorthand (incl. get/set —
            // `ast.objlit_method_exprs`) are FUNCTION-this: §10.2.1.2
            // binds their `this` at their own call site, so the
            // body's `__this` is not free in the enclosing scope
            // (same boundary as the Stmt::FnDecl arm). A plain arrow
            // stays lexical — its `__this` keeps riding up to the
            // enclosing function, which is what hands an arrow inside
            // a promoted fn-expr its receiver.
            if ast.fn_expr_exprs.contains(&eid) || ast.objlit_method_exprs.contains(&eid) {
                bound.push("__this".into());
            }
            hoist_fn_decl_names(body, bound);
            for s in body {
                walk_stmt(ast, s, bound, out);
            }
            bound.truncate(saved);
        }
        Expr::Closure { captures, .. } => {
            // Already lifted (the arena is walked in index order, so an
            // inner lambda is a Closure by the time its encloser's
            // captures are computed): the captures referenced by an
            // already-lifted closure are themselves free in the current
            // arrow body if not bound here — EXCEPT `__this` on a
            // function-this closure (a marked fn-expr or an objlit
            // method shorthand; the eid survives the in-place lift, so
            // the marker sets still answer). That capture is the
            // promote protocol's marker — `fnexpr_this` /
            // `objlit_nominal` later turn it into a receiver param —
            // not a lexical need the enclosing scope must supply;
            // riding it up left the encloser with a stale `__this`
            // capture after the inner promote (unknown-identifier
            // reject). An ARROW's `__this` capture stays lexical and
            // keeps riding up.
            for c in captures {
                if c == "__this"
                    && (ast.fn_expr_exprs.contains(&eid) || ast.objlit_method_exprs.contains(&eid))
                {
                    continue;
                }
                if !bound.contains(c) && !out.contains(c) {
                    out.push(c.clone());
                }
            }
        }
        // M5.1 — by the time arrow-fn lifting runs, classes have already
        // been desugared to functions (and `this` to `__this`). These
        // arms guard against an arrow body that lexically nests inside a
        // class method whose desugar hasn't completed; in practice we
        // run desugar_classes before lift_arrow_fns, so they're inert.
        Expr::This | Expr::NewTarget => {}
        Expr::New {
            class_name, args, ..
        } => {
            // Pre-desugar, `new C()` carries its class name as a String
            // field, not an Ident — report it like one. The module
            // resolver's hidden-dep census is the pass that meets this
            // shape; the arrow-lift caller runs after desugar_classes /
            // builtin-new rewrites, where class News have become
            // factory calls.
            if !is_global_name(class_name)
                && !bound.contains(class_name)
                && !out.contains(class_name)
            {
                out.push(class_name.clone());
            }
            for a in args {
                walk_expr(ast, *a, bound, out);
            }
        }
        Expr::Super { args } => {
            for a in args {
                walk_expr(ast, *a, bound, out);
            }
        }
        Expr::NewDynamic { callee, args } => {
            walk_expr(ast, *callee, bound, out);
            for a in args {
                walk_expr(ast, *a, bound, out);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            walk_expr(ast, *cond, bound, out);
            walk_expr(ast, *then_branch, bound, out);
            walk_expr(ast, *else_branch, bound, out);
        }
        Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr }
        | Expr::As { expr, .. } => walk_expr(ast, *expr, bound, out),
        Expr::InstanceOf { expr, rhs } => {
            walk_expr(ast, *expr, bound, out);
            // A BARE name on the right is resolved by the lowering's
            // static ladder, from the NAME — no value is ever
            // materialised, so it is not a free variable and must not
            // become a capture. (It could not be one before either:
            // the target used to be a `String`, invisible here.) A
            // larger target IS an expression and walks normally, which
            // is the whole point of widening this node.
            walk_expr(ast, *rhs, bound, out);
        }
        Expr::Sequence { left, right } => {
            walk_expr(ast, *left, bound, out);
            walk_expr(ast, *right, bound, out);
        }
        Expr::Nullish { lhs, rhs } => {
            walk_expr(ast, *lhs, bound, out);
            walk_expr(ast, *rhs, bound, out);
        }
        Expr::OptChain { obj, .. } => walk_expr(ast, *obj, bound, out),
        Expr::OptIndex { obj, index } => {
            walk_expr(ast, *obj, bound, out);
            walk_expr(ast, *index, bound, out);
        }
        Expr::OptCall { callee, args } => {
            walk_expr(ast, *callee, bound, out);
            for a in args {
                walk_expr(ast, *a, bound, out);
            }
        }
        Expr::PostIncr { target, .. } => walk_expr(ast, *target, bound, out),
    }
}
