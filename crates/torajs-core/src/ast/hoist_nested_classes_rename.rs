//! α-rename walker: rewrite every reference to `old` inside a
//! statement subtree to `new` — `Ident`, `New { class_name }` and
//! sibling `extends` clauses. The
//! walk mirrors the free-vars traversal's coverage but mutates the
//! arena in place; ArrowFn bodies are taken out and re-seated so the
//! arena borrow never overlaps the recursion.

use super::*;

pub(super) fn rename_in_stmts(ast: &mut Ast, stmts: &mut [Stmt], old: &str, new: &str) {
    for s in stmts.iter_mut() {
        rename_in_stmt(ast, s, old, new);
    }
}

fn rename_in_stmt(ast: &mut Ast, s: &mut Stmt, old: &str, new: &str) {
    match s {
        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Yield(e) | Stmt::Throw(e) => {
            rename_in_expr(ast, *e, old, new)
        }
        Stmt::YieldInto { value, .. } => rename_in_expr(ast, *value, old, new),
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => {
            rename_in_expr(ast, *init, old, new)
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            rename_in_expr(ast, *cond, old, new);
            rename_in_stmt(ast, then_branch, old, new);
            if let Some(eb) = else_branch {
                rename_in_stmt(ast, eb, old, new);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            rename_in_expr(ast, *cond, old, new);
            rename_in_stmt(ast, body, old, new);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                rename_in_stmt(ast, i, old, new);
            }
            for e in [cond.as_ref(), step.as_ref()].into_iter().flatten() {
                rename_in_expr(ast, *e, old, new);
            }
            rename_in_stmt(ast, body, old, new);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            rename_in_expr(ast, *parent, old, new);
            rename_in_expr(ast, *sep, old, new);
            rename_in_stmt(ast, body, old, new);
        }
        Stmt::ForOf {
            elem_expr,
            body,
            forin_obj,
            ..
        } => {
            rename_in_expr(ast, *elem_expr, old, new);
            if let Some(o) = forin_obj {
                rename_in_expr(ast, *o, old, new);
            }
            rename_in_stmt(ast, body, old, new);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            rename_in_expr(ast, *scrutinee, old, new);
            for c in cases {
                rename_in_expr(ast, c.value, old, new);
                rename_in_stmts(ast, &mut c.body, old, new);
            }
            if let Some(d) = default {
                rename_in_stmts(ast, d, old, new);
            }
        }
        Stmt::Block(v) | Stmt::Multi(v) => rename_in_stmts(ast, v, old, new),
        Stmt::Labeled { body, .. } => rename_in_stmt(ast, body, old, new),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            rename_in_stmts(ast, body, old, new);
            rename_in_stmts(ast, catch_body, old, new);
            if let Some(fb) = finally_body {
                rename_in_stmts(ast, fb, old, new);
            }
        }
        Stmt::FnDecl { body, .. } => rename_in_stmts(ast, body, old, new),
        Stmt::ClassDecl {
            name,
            parent,
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            if name == old {
                *name = new.to_string();
            }
            // The heritage is an expression (RFC 20260815): a bare
            // `Expr::Ident` root is renamed by the same walk that
            // renames every identifier inside a non-Ident heritage.
            if let Some(pid) = parent {
                rename_in_expr(ast, *pid, old, new);
            }
            if let Some(c) = ctor {
                rename_in_stmts(ast, &mut c.body, old, new);
            }
            for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                rename_in_stmts(ast, &mut m.body, old, new);
            }
            for si in static_init {
                match si {
                    StaticInit::Field(f) => rename_in_expr(ast, f.init, old, new),
                    StaticInit::Block(v) => rename_in_stmts(ast, v, old, new),
                }
            }
        }
        Stmt::ExportDecl {
            inner,
            default_expr,
            ..
        } => {
            if let Some(inner) = inner {
                rename_in_stmt(ast, inner, old, new);
            }
            if let Some(e) = default_expr {
                rename_in_expr(ast, *e, old, new);
            }
        }
        Stmt::Return(None)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::TypeDecl { .. }
        | Stmt::ImportDecl { .. } => {}
    }
}

fn rename_in_expr(ast: &mut Ast, eid: ExprId, old: &str, new: &str) {
    // Read the child ids out first so the arena borrow releases
    // before recursing.
    enum Kids {
        None,
        One(ExprId),
        Two(ExprId, ExprId),
        Three(ExprId, ExprId, ExprId),
        Many(Vec<ExprId>),
        Arrow,
    }
    let kids = match &mut ast.exprs[eid.0 as usize] {
        Expr::Ident(n) => {
            if n == old {
                *n = new.to_string();
            }
            Kids::None
        }
        Expr::New {
            class_name, args, ..
        } => {
            if class_name == old {
                *class_name = new.to_string();
            }
            Kids::Many(args.clone())
        }
        // The target is an expression now, so a bare class name in it
        // is an `Expr::Ident` the arm above already renames — nothing
        // to do here beyond descending into both children.
        Expr::InstanceOf { expr, rhs } => Kids::Two(*expr, *rhs),
        Expr::Elision
        | Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::This
        | Expr::NewTarget
        | Expr::Closure { .. } => Kids::None,
        Expr::BinOp { left, right, .. }
        | Expr::Sequence { left, right }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        }
        | Expr::Assign {
            target: left,
            value: right,
        }
        | Expr::Index {
            obj: left,
            index: right,
        }
        | Expr::OptIndex {
            obj: left,
            index: right,
        } => Kids::Two(*left, *right),
        Expr::Unary { expr, .. }
        | Expr::Member { obj: expr, .. }
        | Expr::OptChain { obj: expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr }
        | Expr::As { expr, .. }
        | Expr::PostIncr { target: expr, .. } => Kids::One(*expr),
        Expr::Call { callee, args }
        | Expr::OptCall { callee, args }
        | Expr::NewDynamic { callee, args } => {
            let mut v = vec![*callee];
            v.extend(args.iter().copied());
            Kids::Many(v)
        }
        Expr::Super { args } => Kids::Many(args.clone()),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => Kids::Three(*cond, *then_branch, *else_branch),
        Expr::Array(elems) => Kids::Many(elems.clone()),
        Expr::ObjectLit { fields } => Kids::Many(fields.iter().map(|(_, e)| *e).collect()),
        Expr::ArrowFn { .. } => Kids::Arrow,
    };
    match kids {
        Kids::None => {}
        Kids::One(a) => rename_in_expr(ast, a, old, new),
        Kids::Two(a, b) => {
            rename_in_expr(ast, a, old, new);
            rename_in_expr(ast, b, old, new);
        }
        Kids::Three(a, b, c) => {
            rename_in_expr(ast, a, old, new);
            rename_in_expr(ast, b, old, new);
            rename_in_expr(ast, c, old, new);
        }
        Kids::Many(v) => {
            for e in v {
                rename_in_expr(ast, e, old, new);
            }
        }
        Kids::Arrow => {
            let mut body = match &mut ast.exprs[eid.0 as usize] {
                Expr::ArrowFn { body, .. } => std::mem::take(body),
                _ => unreachable!(),
            };
            rename_in_stmts(ast, &mut body, old, new);
            if let Expr::ArrowFn { body: b, .. } = &mut ast.exprs[eid.0 as usize] {
                *b = body;
            }
        }
    }
}
