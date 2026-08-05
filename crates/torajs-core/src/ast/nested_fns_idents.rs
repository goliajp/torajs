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
    arrows: bool,
) {
    for s in body.iter_mut() {
        rewrite_idents_in_stmt(ast, s, renames, arrows);
    }
}

pub(super) fn rewrite_idents_in_stmt(
    ast: &mut Ast,
    s: &mut Stmt,
    renames: &HashMap<String, String>,
    arrows: bool,
) {
    match s {
        Stmt::Expr(eid) => rewrite_idents_in_expr(ast, *eid, renames, arrows),
        Stmt::LetDecl { init, .. } => rewrite_idents_in_expr(ast, *init, renames, arrows),
        Stmt::Return(Some(eid)) => rewrite_idents_in_expr(ast, *eid, renames, arrows),
        Stmt::Throw(eid) => rewrite_idents_in_expr(ast, *eid, renames, arrows),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            rewrite_idents_in_expr(ast, *cond, renames, arrows);
            rewrite_idents_in_stmt(ast, then_branch, renames, arrows);
            if let Some(eb) = else_branch.as_deref_mut() {
                rewrite_idents_in_stmt(ast, eb, renames, arrows);
            }
        }
        Stmt::Labeled { body, .. } => rewrite_idents_in_stmt(ast, body, renames, arrows),
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            rewrite_idents_in_expr(ast, *cond, renames, arrows);
            rewrite_idents_in_stmt(ast, body, renames, arrows);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init.as_deref_mut() {
                rewrite_idents_in_stmt(ast, i, renames, arrows);
            }
            if let Some(c) = cond {
                rewrite_idents_in_expr(ast, *c, renames, arrows);
            }
            if let Some(st) = step {
                rewrite_idents_in_expr(ast, *st, renames, arrows);
            }
            rewrite_idents_in_stmt(ast, body, renames, arrows);
        }
        Stmt::Block(b) | Stmt::Multi(b) => {
            for s2 in b.iter_mut() {
                rewrite_idents_in_stmt(ast, s2, renames, arrows);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s2 in body.iter_mut() {
                rewrite_idents_in_stmt(ast, s2, renames, arrows);
            }
            for s2 in catch_body.iter_mut() {
                rewrite_idents_in_stmt(ast, s2, renames, arrows);
            }
            if let Some(fb) = finally_body {
                for s2 in fb.iter_mut() {
                    rewrite_idents_in_stmt(ast, s2, renames, arrows);
                }
            }
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            rewrite_idents_in_expr(ast, *scrutinee, renames, arrows);
            for case in cases.iter_mut() {
                rewrite_idents_in_expr(ast, case.value, renames, arrows);
                for s2 in case.body.iter_mut() {
                    rewrite_idents_in_stmt(ast, s2, renames, arrows);
                }
            }
            if let Some(dflt) = default {
                for s2 in dflt.iter_mut() {
                    rewrite_idents_in_stmt(ast, s2, renames, arrows);
                }
            }
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            rewrite_idents_in_expr(ast, *parent, renames, arrows);
            rewrite_idents_in_expr(ast, *sep, renames, arrows);
            rewrite_idents_in_stmt(ast, body, renames, arrows);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            rewrite_idents_in_expr(ast, *elem_expr, renames, arrows);
            rewrite_idents_in_stmt(ast, body, renames, arrows);
        }
        Stmt::Yield(eid) | Stmt::YieldInto { value: eid, .. } => {
            rewrite_idents_in_expr(ast, *eid, renames, arrows);
        }
        _ => {}
    }
}

pub(super) fn rewrite_idents_in_expr(
    ast: &mut Ast,
    eid: ExprId,
    renames: &HashMap<String, String>,
    arrows: bool,
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
            // An arrow body is a separate scope, but a nested scope
            // SEES its enclosing bindings — and renaming a nested
            // `function f` is a rename of one, WHEN the rename is a
            // hoist into the enclosing function scope (`arrows`).
            // Pass 2's module-top-level blocks are the other case: in
            // strict mode `{ function f(){} }` at global scope is
            // block-scoped and nothing outside the block resolves it,
            // so the descent must NOT happen there — doing it made
            // `assert.throws(ReferenceError, function() { f; })`
            // resolve and stop throwing (test262
            // `global-code/{block,switch-case,switch-dflt}-decl-strict`).
            // Skipping the descent
            // left `(x) => step(x)` calling a `step` that no longer
            // exists under that name once the declaration lifted to
            // `__nested_<parent>_step_N`; `lift_arrow_fns` moves the
            // arrow to top level before this pass runs, so nothing
            // downstream was in a position to fix it either
            // ("handled by their own pass" was not true of this
            // case). Names the arrow rebinds are dropped from the map
            // first — those references are its own.
            Expr::ArrowFn { params, body, .. } if arrows => {
                let sub: HashMap<String, String> = renames
                    .iter()
                    .filter(|(k, _)| !arrow_rebinds(&params, &body, k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                if !sub.is_empty() {
                    let mut inner = body;
                    rewrite_idents_in_body(ast, &mut inner, &sub, arrows);
                    if let Expr::ArrowFn { body: slot, .. } = &mut ast.exprs[id.0 as usize] {
                        *slot = inner;
                    }
                }
            }
            _ => {}
        }
    }
}

/// True when an arrow rebinds `name` itself — as a parameter, or as a
/// declaration at the top of its body. Such a reference belongs to the
/// arrow, not to the enclosing scope whose declaration is being
/// renamed, so it must keep the original name.
///
/// Deliberately shallow: a rebinding nested deeper than the arrow's
/// own statement list shadows only within that inner block, where a
/// reference to the outer name cannot appear anyway.
///
/// Shared with `desugar_generators_rewrite`, which descends into arrow
/// bodies for the same reason and needs the same exclusion.
pub(super) fn arrow_rebinds(params: &[super::Param], body: &[Stmt], name: &str) -> bool {
    params.iter().any(|p| p.name == name)
        || body.iter().any(|s| match s {
            Stmt::LetDecl { name: n, .. } | Stmt::FnDecl { name: n, .. } => n == name,
            _ => false,
        })
}
