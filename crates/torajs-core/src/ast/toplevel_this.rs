//! Top-level `this` — cluster #5 (test262), rotation 235.
//!
//! `desugar_classes` rewrites every `Expr::This` to `Ident("__this")`
//! unconditionally; class members and blade-1 plain fns get a binder
//! for it, but the TOP LEVEL never did — `var g = this` /
//! `verifyProperty(this, "Symbol", …)` rejected with
//! `unknown identifier __this` (475-case bare shape in the census).
//!
//! bun treats a `.ts` entry as a CommonJS-flavored module whose
//! top-level `this` is the (initially empty) exports object, so the
//! module-this materializes as exactly that: one fresh `{}` dynobj
//! for the whole program (`this === this` holds), spelled
//! `__module_this` so nothing else resolves to it by accident — in
//! particular a plain fn body's `__this` keeps its blade-1 param
//! route (or stays loud), and the `__` prefix keeps the binding out
//! of the K.3b global lane.
//!
//! The walk is positional, not name-based: only `Ident("__this")`
//! reachable from top-level statements WITHOUT crossing a function
//! boundary rewrites. A true arrow inherits the lexical top-level
//! `this` (§8.3.4) and walks through; a function EXPRESSION owns its
//! `this` (the parser parks both in `Expr::ArrowFn`, telling them
//! apart via `ast.fn_expr_exprs`) and does not. Statement / expr
//! shapes the walker doesn't know stay un-rewritten — that path
//! keeps today's loud reject, never a silent wrong.
//!
//! Runs after `bind_this_param` (order only matters for readability:
//! neither pass touches the other's sites).

use super::{Ast, Expr, ExprId, Stmt};

const MODULE_THIS: &str = "__module_this";

pub fn rewrite_toplevel_this(ast: &mut Ast) {
    let mut hits: Vec<ExprId> = Vec::new();
    for s in &ast.stmts {
        if matches!(s, Stmt::FnDecl { .. }) {
            continue;
        }
        collect_stmt(ast, s, &mut hits);
    }
    if hits.is_empty() {
        return;
    }
    let obj = ast.add_expr(Expr::ObjectLit { fields: vec![] });
    ast.stmts.insert(
        0,
        Stmt::LetDecl {
            mutable: false,
            name: MODULE_THIS.into(),
            type_ann: Some("any".into()),
            init: obj,
            is_var: false,
        },
    );
    for eid in hits {
        ast.exprs[eid.0 as usize] = Expr::Ident(MODULE_THIS.into());
    }
}

fn collect_stmt(ast: &Ast, s: &Stmt, out: &mut Vec<ExprId>) {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => collect_expr(ast, *e, out),
        Stmt::LetDecl { init, .. } => collect_expr(ast, *init, out),
        Stmt::YieldInto { value, .. } => collect_expr(ast, *value, out),
        Stmt::Return(e) => {
            if let Some(e) = e {
                collect_expr(ast, *e, out);
            }
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_expr(ast, *cond, out);
            collect_stmt(ast, then_branch, out);
            if let Some(eb) = else_branch {
                collect_stmt(ast, eb, out);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            collect_expr(ast, *cond, out);
            collect_stmt(ast, body, out);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            collect_expr(ast, *scrutinee, out);
            for c in cases {
                collect_expr(ast, c.value, out);
                for cs in &c.body {
                    collect_stmt(ast, cs, out);
                }
            }
            if let Some(d) = default {
                for ds in d {
                    collect_stmt(ast, ds, out);
                }
            }
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                collect_stmt(ast, i, out);
            }
            if let Some(c) = cond {
                collect_expr(ast, *c, out);
            }
            if let Some(st) = step {
                collect_expr(ast, *st, out);
            }
            collect_stmt(ast, body, out);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            collect_expr(ast, *parent, out);
            collect_expr(ast, *sep, out);
            collect_stmt(ast, body, out);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            collect_expr(ast, *elem_expr, out);
            collect_stmt(ast, body, out);
        }
        Stmt::Labeled { body, .. } => collect_stmt(ast, body, out),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body {
                collect_stmt(ast, s, out);
            }
            for s in catch_body {
                collect_stmt(ast, s, out);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    collect_stmt(ast, s, out);
                }
            }
        }
        Stmt::Block(v) | Stmt::Multi(v) => {
            for s in v {
                collect_stmt(ast, s, out);
            }
        }
        // FnDecl bodies own their `this`; type-only / module decls
        // carry no runtime `this`. Anything else stays un-walked —
        // loud, never silently wrong.
        _ => {}
    }
}

fn collect_expr(ast: &Ast, eid: ExprId, out: &mut Vec<ExprId>) {
    match ast.get_expr(eid) {
        Expr::Ident(n) if n == "__this" => out.push(eid),
        // A true arrow inherits the lexical top-level `this`; a
        // function EXPRESSION (same variant, `fn_expr_exprs` marked)
        // owns its `this`, and an object-literal METHOD / accessor /
        // fn-expr-field value (`objlit_method_exprs` — the parser
        // parks all of them in ArrowFn too) binds `this` to the
        // receiver (§10.2.1.2) — both stay out (gate caught 21
        // objlit fixtures going silent-wrong when this walked in).
        Expr::ArrowFn { body, .. } => {
            if !ast.fn_expr_exprs.contains(&eid) && !ast.objlit_method_exprs.contains(&eid) {
                for s in body {
                    collect_stmt(ast, s, out);
                }
            }
        }
        Expr::BinOp { left, right, .. }
        | Expr::Sequence { left, right }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        } => {
            collect_expr(ast, *left, out);
            collect_expr(ast, *right, out);
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr }
        | Expr::As { expr, .. } => collect_expr(ast, *expr, out),
        Expr::InstanceOf { expr, rhs } => {
            collect_expr(ast, *expr, out);
            collect_expr(ast, *rhs, out);
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => collect_expr(ast, *obj, out),
        Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
            collect_expr(ast, *callee, out);
            for a in args {
                collect_expr(ast, *a, out);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                collect_expr(ast, *a, out);
            }
        }
        Expr::Assign { target, value } => {
            collect_expr(ast, *target, out);
            collect_expr(ast, *value, out);
        }
        Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
            collect_expr(ast, *obj, out);
            collect_expr(ast, *index, out);
        }
        Expr::Array(els) => {
            for e in els {
                collect_expr(ast, *e, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, v) in fields {
                collect_expr(ast, *v, out);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_expr(ast, *cond, out);
            collect_expr(ast, *then_branch, out);
            collect_expr(ast, *else_branch, out);
        }
        Expr::PostIncr { target, .. } => collect_expr(ast, *target, out),
        // Closure shouldn't exist pre-lift; literals / Ident(other)
        // carry no `this`. Unknown shapes stay loud.
        _ => {}
    }
}
