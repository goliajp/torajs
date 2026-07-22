//! Ident-rewrite walk for the nested-fn hoisting pass — split from
//! `nested_fns` (labeled break/continue work) to keep that file under
//! the size limit. Rewrites references to a lifted nested fn's original
//! name into its mangled top-level name across a statement subtree.

use super::{Ast, Expr, ExprId, Stmt};
use std::collections::HashMap;

pub(super) fn rewrite_idents_in_body(
    ast: &mut Ast,
    body: &mut Vec<Stmt>,
    renames: &HashMap<String, String>,
) {
    for s in body.iter_mut() {
        rewrite_idents_in_stmt(ast, s, renames);
    }
}

pub(super) fn rewrite_idents_in_stmt(
    ast: &mut Ast,
    s: &mut Stmt,
    renames: &HashMap<String, String>,
) {
    match s {
        Stmt::Expr(eid) => rewrite_idents_in_expr(ast, *eid, renames),
        Stmt::LetDecl { init, .. } => rewrite_idents_in_expr(ast, *init, renames),
        Stmt::Return(Some(eid)) => rewrite_idents_in_expr(ast, *eid, renames),
        Stmt::Throw(eid) => rewrite_idents_in_expr(ast, *eid, renames),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            rewrite_idents_in_expr(ast, *cond, renames);
            rewrite_idents_in_stmt(ast, then_branch, renames);
            if let Some(eb) = else_branch.as_deref_mut() {
                rewrite_idents_in_stmt(ast, eb, renames);
            }
        }
        Stmt::Labeled { body, .. } => rewrite_idents_in_stmt(ast, body, renames),
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            rewrite_idents_in_expr(ast, *cond, renames);
            rewrite_idents_in_stmt(ast, body, renames);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init.as_deref_mut() {
                rewrite_idents_in_stmt(ast, i, renames);
            }
            if let Some(c) = cond {
                rewrite_idents_in_expr(ast, *c, renames);
            }
            if let Some(st) = step {
                rewrite_idents_in_expr(ast, *st, renames);
            }
            rewrite_idents_in_stmt(ast, body, renames);
        }
        Stmt::Block(b) | Stmt::Multi(b) => {
            for s2 in b.iter_mut() {
                rewrite_idents_in_stmt(ast, s2, renames);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s2 in body.iter_mut() {
                rewrite_idents_in_stmt(ast, s2, renames);
            }
            for s2 in catch_body.iter_mut() {
                rewrite_idents_in_stmt(ast, s2, renames);
            }
            if let Some(fb) = finally_body {
                for s2 in fb.iter_mut() {
                    rewrite_idents_in_stmt(ast, s2, renames);
                }
            }
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            rewrite_idents_in_expr(ast, *scrutinee, renames);
            for case in cases.iter_mut() {
                rewrite_idents_in_expr(ast, case.value, renames);
                for s2 in case.body.iter_mut() {
                    rewrite_idents_in_stmt(ast, s2, renames);
                }
            }
            if let Some(dflt) = default {
                for s2 in dflt.iter_mut() {
                    rewrite_idents_in_stmt(ast, s2, renames);
                }
            }
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            rewrite_idents_in_expr(ast, *parent, renames);
            rewrite_idents_in_expr(ast, *sep, renames);
            rewrite_idents_in_stmt(ast, body, renames);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            rewrite_idents_in_expr(ast, *elem_expr, renames);
            rewrite_idents_in_stmt(ast, body, renames);
        }
        Stmt::Yield(eid) | Stmt::YieldInto { value: eid, .. } => {
            rewrite_idents_in_expr(ast, *eid, renames);
        }
        _ => {}
    }
}

pub(super) fn rewrite_idents_in_expr(
    ast: &mut Ast,
    eid: ExprId,
    renames: &HashMap<String, String>,
) {
    use std::collections::HashSet;
    let mut seen: HashSet<ExprId> = HashSet::new();
    let mut stack: Vec<ExprId> = vec![eid];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let new_name = if let Expr::Ident(n) = &ast.exprs[id.0 as usize] {
            renames.get(n).cloned()
        } else {
            None
        };
        if let Some(nm) = new_name {
            ast.exprs[id.0 as usize] = Expr::Ident(nm);
            continue;
        }
        match ast.exprs[id.0 as usize].clone() {
            Expr::BinOp { left, right, .. } => {
                stack.push(left);
                stack.push(right);
            }
            Expr::Unary { expr, .. }
            | Expr::TypeOf { expr }
            | Expr::Spread { expr }
            | Expr::PostIncr { target: expr, .. } => {
                stack.push(expr);
            }
            Expr::Call { callee, args } => {
                stack.push(callee);
                for a in args {
                    stack.push(a);
                }
            }
            Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
                stack.push(obj);
            }
            Expr::OptIndex { obj, index } | Expr::Index { obj, index } => {
                stack.push(obj);
                stack.push(index);
            }
            Expr::OptCall { callee, args } => {
                stack.push(callee);
                for a in args {
                    stack.push(a);
                }
            }
            Expr::Assign { target, value } => {
                stack.push(target);
                stack.push(value);
            }
            Expr::Array(els) => {
                for e in els {
                    stack.push(e);
                }
            }
            Expr::ObjectLit { fields } => {
                for (_, e) in fields {
                    stack.push(e);
                }
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                stack.push(cond);
                stack.push(then_branch);
                stack.push(else_branch);
            }
            Expr::Nullish { lhs, rhs }
            | Expr::Sequence {
                left: lhs,
                right: rhs,
            } => {
                stack.push(lhs);
                stack.push(rhs);
            }
            Expr::New { args, .. } | Expr::Super { args } => {
                for a in args {
                    stack.push(a);
                }
            }
            Expr::InstanceOf { expr, .. } => {
                stack.push(expr);
            }
            // ArrowFn / Closure bodies are separate scopes — don't
            // descend (handled by lift_arrow_fns + their own pass).
            _ => {}
        }
    }
}
