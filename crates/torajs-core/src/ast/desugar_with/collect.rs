//! The two walks the `with` desugar runs over a body: which names the
//! body binds LEXICALLY (those shadow the object), and where every
//! remaining identifier occurrence sits (which decides its rewrite).
//!
//! Both are deliberately conservative in the same direction: a shape
//! this file does not recognise becomes a `Position::Refused`, so an
//! unhandled corner is a diagnostic rather than a name that quietly
//! resolved to the lexical binding.

use std::collections::HashSet;

use super::Position;
use crate::ast::{Ast, Expr, ExprId, Stmt};

/// The `&mut Stmt` children the depth-first rewrite descends into,
/// so an inner `with` is finished before the outer one starts.
pub(crate) fn stmt_children(s: &mut Stmt) -> Vec<&mut Stmt> {
    match s {
        Stmt::Block(v) | Stmt::Multi(v) => v.iter_mut().collect(),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            let mut v: Vec<&mut Stmt> = vec![then_branch.as_mut()];
            if let Some(e) = else_branch.as_mut() {
                v.push(e.as_mut());
            }
            v
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Labeled { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. } => vec![body.as_mut()],
        Stmt::For { init, body, .. } => {
            let mut v: Vec<&mut Stmt> = Vec::new();
            if let Some(i) = init.as_mut() {
                v.push(i.as_mut());
            }
            v.push(body.as_mut());
            v
        }
        Stmt::Switch { cases, default, .. } => {
            let mut v: Vec<&mut Stmt> = Vec::new();
            for c in cases.iter_mut() {
                v.extend(c.body.iter_mut());
            }
            if let Some(d) = default.as_mut() {
                v.extend(d.iter_mut());
            }
            v
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            let mut v: Vec<&mut Stmt> = body.iter_mut().collect();
            v.extend(catch_body.iter_mut());
            if let Some(f) = finally_body.as_mut() {
                v.extend(f.iter_mut());
            }
            v
        }
        Stmt::FnDecl { body, .. } => body.iter_mut().collect(),
        _ => Vec::new(),
    }
}

/// Collect the body's lexical binders into `bound` and every
/// identifier occurrence into `sites`.
///
/// `var` names are NOT collected: they hoist past the object record,
/// so `with (o) { var v = 1 }` still means `o.v` when `o` has `v`.
/// `let` / `const` / `class` / catch parameters are, because their
/// records sit in front of the object's.
pub(crate) fn collect_stmt(
    ast: &Ast,
    s: &Stmt,
    bound: &mut HashSet<String>,
    sites: &mut Vec<(ExprId, Position)>,
    err: &mut Option<String>,
) {
    match s {
        Stmt::LetDecl {
            name, init, is_var, ..
        } => {
            if *is_var {
                // `var` does NOT shadow the object — it hoists to the
                // function scope, which sits BEHIND the object record.
                // So `with (o) { var v = 2 }` writes `o.v` when `o`
                // carries `v`, and leaves the hoisted binding
                // undefined (bun: `2 undefined`; the unrewritten
                // reading is `1 2`). The initialiser is an assignment
                // evaluated in the with scope, so it belongs to the
                // write knife — refused until then rather than
                // silently landing on the hoisted binding.
                refuse(err, "a `var` declaration");
            } else {
                bound.insert(name.clone());
            }
            collect_expr(ast, *init, sites, err);
        }
        Stmt::ClassDecl { name, .. } => {
            bound.insert(name.clone());
            refuse(err, "a class declaration");
        }
        Stmt::FnDecl { name, .. } => {
            bound.insert(name.clone());
            // §14.11 with a function inside it: the body's free names
            // resolve when the function is CALLED, so the object has
            // to be captured by the closure — RFC 20260814 刀 4.
            refuse(err, "a nested function body");
        }
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => collect_expr(ast, *e, sites, err),
        Stmt::Return(o) => {
            if let Some(e) = o {
                collect_expr(ast, *e, sites, err);
            }
        }
        Stmt::YieldInto { value, .. } => collect_expr(ast, *value, sites, err),
        Stmt::If { cond, .. } | Stmt::While { cond, .. } | Stmt::DoWhile { cond, .. } => {
            collect_expr(ast, *cond, sites, err);
        }
        Stmt::Switch {
            scrutinee, cases, ..
        } => {
            collect_expr(ast, *scrutinee, sites, err);
            for c in cases {
                collect_expr(ast, c.value, sites, err);
            }
        }
        Stmt::For { cond, step, .. } => {
            if let Some(c) = cond {
                collect_expr(ast, *c, sites, err);
            }
            if let Some(st) = step {
                collect_expr(ast, *st, sites, err);
            }
        }
        Stmt::ForOf {
            var_name,
            src_ident,
            i_ident,
            elem_expr,
            ..
        } => {
            // The parser already hoisted the source to `src_ident`
            // and minted the counter, so all three names are the
            // loop's own — bound, not free.
            bound.insert(var_name.clone());
            bound.insert(src_ident.clone());
            bound.insert(i_ident.clone());
            collect_expr(ast, *elem_expr, sites, err);
        }
        Stmt::ForOfSplitIter {
            var_name,
            parent,
            sep,
            ..
        } => {
            bound.insert(var_name.clone());
            collect_expr(ast, *parent, sites, err);
            collect_expr(ast, *sep, sites, err);
        }
        Stmt::Try { catch_param, .. } => {
            if let Some(p) = catch_param {
                bound.insert(p.clone());
            }
        }
        Stmt::UsingDecl { name, init, .. } => {
            bound.insert(name.clone());
            collect_expr(ast, *init, sites, err);
        }
        Stmt::Block(_) | Stmt::Multi(_) | Stmt::Labeled { .. } => {}
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::TypeDecl { .. } => {}
        Stmt::ImportDecl { .. } | Stmt::ExportDecl { .. } => {}
    }
    for child in stmt_children_ref(s) {
        collect_stmt(ast, child, bound, sites, err);
    }
}

/// Read-only twin of [`super::stmt_children`] — the collect walk needs
/// the same shape without the mutable borrow.
fn stmt_children_ref(s: &Stmt) -> Vec<&Stmt> {
    match s {
        Stmt::Block(v) | Stmt::Multi(v) => v.iter().collect(),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            let mut v: Vec<&Stmt> = vec![then_branch.as_ref()];
            if let Some(e) = else_branch.as_ref() {
                v.push(e.as_ref());
            }
            v
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Labeled { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. } => vec![body.as_ref()],
        Stmt::For { init, body, .. } => {
            let mut v: Vec<&Stmt> = Vec::new();
            if let Some(i) = init.as_ref() {
                v.push(i.as_ref());
            }
            v.push(body.as_ref());
            v
        }
        Stmt::Switch { cases, default, .. } => {
            let mut v: Vec<&Stmt> = Vec::new();
            for c in cases {
                v.extend(c.body.iter());
            }
            if let Some(d) = default.as_ref() {
                v.extend(d.iter());
            }
            v
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            let mut v: Vec<&Stmt> = body.iter().collect();
            v.extend(catch_body.iter());
            if let Some(f) = finally_body.as_ref() {
                v.extend(f.iter());
            }
            v
        }
        _ => Vec::new(),
    }
}

fn refuse(err: &mut Option<String>, what: &'static str) {
    err.get_or_insert(format!(
        "{what} inside a `with` body is not yet supported \
         (RFC 20260814, ES §14.11)"
    ));
}

/// Walk one expression subtree, recording each `Ident` with the
/// position that decides its rewrite.
fn collect_expr(
    ast: &Ast,
    eid: ExprId,
    sites: &mut Vec<(ExprId, Position)>,
    err: &mut Option<String>,
) {
    let push_read = |e: ExprId, sites: &mut Vec<(ExprId, Position)>| {
        if matches!(ast.get_expr(e), Expr::Ident(_)) {
            sites.push((e, Position::Read));
        }
    };
    match ast.get_expr(eid) {
        Expr::Ident(_) => {}
        Expr::Call { callee, args } => {
            if matches!(ast.get_expr(*callee), Expr::Ident(_)) {
                sites.push((*callee, Position::Callee(eid)));
            } else {
                collect_expr(ast, *callee, sites, err);
            }
            for a in args {
                push_read(*a, sites);
                collect_expr(ast, *a, sites, err);
            }
            return;
        }
        Expr::Assign { target, value } => {
            if matches!(ast.get_expr(*target), Expr::Ident(_)) {
                sites.push((*target, Position::Refused("an assignment")));
            } else {
                collect_expr(ast, *target, sites, err);
            }
            push_read(*value, sites);
            collect_expr(ast, *value, sites, err);
            return;
        }
        Expr::PostIncr { target, .. } => {
            if matches!(ast.get_expr(*target), Expr::Ident(_)) {
                sites.push((*target, Position::Wrapping(eid)));
            } else {
                collect_expr(ast, *target, sites, err);
            }
            return;
        }
        Expr::TypeOf { expr } => {
            if matches!(ast.get_expr(*expr), Expr::Ident(_)) {
                sites.push((*expr, Position::Wrapping(eid)));
            } else {
                collect_expr(ast, *expr, sites, err);
            }
            return;
        }
        Expr::Delete { expr } => {
            if matches!(ast.get_expr(*expr), Expr::Ident(_)) {
                sites.push((*expr, Position::Refused("`delete`")));
            } else {
                collect_expr(ast, *expr, sites, err);
            }
            return;
        }
        Expr::ArrowFn { .. } | Expr::Closure { .. } => {
            // Same reason as a nested `function`: the body's names
            // resolve at call time, so the object has to be captured
            // (RFC 20260814 刀 4).
            refuse(err, "a nested function body");
            return;
        }
        _ => {}
    }
    for c in expr_children(ast, eid) {
        push_read(c, sites);
        collect_expr(ast, c, sites, err);
    }
}

/// Every child ExprId of one node. Written here rather than borrowed
/// from a sibling walk because no shared one exists: the passes that
/// walk expressions each carry their own arm set, tuned to what they
/// are looking for.
fn expr_children(ast: &Ast, eid: ExprId) -> Vec<ExprId> {
    match ast.get_expr(eid) {
        Expr::BinOp { left, right, .. }
        | Expr::Sequence { left, right }
        | Expr::Index {
            obj: left,
            index: right,
        }
        | Expr::OptIndex {
            obj: left,
            index: right,
        }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        }
        | Expr::InstanceOf {
            expr: left,
            rhs: right,
        }
        | Expr::Assign {
            target: left,
            value: right,
        } => vec![*left, *right],
        Expr::Unary { expr, .. }
        | Expr::As { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr } => vec![*expr],
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => vec![*obj],
        Expr::PostIncr { target, .. } => vec![*target],
        Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
            let mut v = vec![*callee];
            v.extend(args.iter().copied());
            v
        }
        Expr::NewDynamic { callee, args, .. } => {
            let mut v = vec![*callee];
            v.extend(args.iter().copied());
            v
        }
        Expr::New { args, .. } | Expr::Super { args, .. } => args.clone(),
        Expr::Array(items) => items.clone(),
        Expr::ObjectLit { fields } => fields.iter().map(|(_, e)| *e).collect(),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => vec![*cond, *then_branch, *else_branch],
        _ => Vec::new(),
    }
}
