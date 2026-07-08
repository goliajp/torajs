//! Nested-fn hoisting pass — chunk 358, extracted from ast.rs.
//!
//! Pub entry `desugar_nested_fns` implements ES Annex B §B.3.3
//! (FunctionDeclarations in IfStatement Blocks). Walks every top-
//! level FnDecl body + module-top-level non-FnDecl stmts, finds
//! nested `function f() {...}` decls inside any block-shape stmt,
//! lifts them to top-level with mangled names
//! `__nested_<parent>_<name>_<uid>`, rewrites references. Six
//! private helpers (`collect_nested_fns_in_stmt/to_lift/in_other`,
//! `rewrite_idents_in_body/stmt/expr`) do the tree walk.

use super::{Ast, Expr, ExprId, Stmt};
use std::collections::HashMap;

/// P3.4 — block-scoped / nested function declaration hoisting per
/// ES spec Annex B §B.3.3 (FunctionDeclarations in IfStatement
/// Blocks). Walks every top-level FnDecl body, finds nested
/// `function f() {...}` declarations inside any block-shape stmt,
/// lifts them to top-level with mangled names
/// `__nested_<parent>_<name>_<uid>`. References to the original
/// name within the parent body get rewritten to the mangled name.
///
/// Captures: this pass only handles the no-capture case. If the
/// nested fn body references parent locals, ssa_lower will fail at
/// the lifted FnDecl's body lower (lifted = top-level scope, can't
/// see parent locals). Real closure-capturing nested fns need the
/// same treatment as arrow fns (lift_arrow_fns adds an `__env`
/// first param + Closure shape) — substrate followup if test262
/// surfaces it.
pub fn desugar_nested_fns(ast: &mut Ast) {
    let mut top = std::mem::take(&mut ast.stmts);
    let mut new_top: Vec<Stmt> = Vec::new();
    let mut counter: u32 = 0;
    // Pass 1 — top-level FnDecl bodies. Walk nested fns inside parent
    // FnDecl bodies (the original P3.4 scope).
    for stmt in top.iter_mut() {
        if let Stmt::FnDecl {
            name: parent_name,
            body,
            ..
        } = stmt
        {
            let parent = parent_name.clone();
            let mut renames: HashMap<String, String> = HashMap::new();
            let mut lifted: Vec<Stmt> = Vec::new();
            collect_nested_fns_to_lift(body, &parent, &mut renames, &mut lifted, &mut counter);
            if !renames.is_empty() {
                rewrite_idents_in_body(ast, body, &renames);
                for lf in lifted.iter_mut() {
                    if let Stmt::FnDecl { body: lb, .. } = lf {
                        rewrite_idents_in_body(ast, lb, &renames);
                    }
                }
            }
            new_top.extend(lifted);
        }
    }
    // P3.4-followup-A — module-top-level blocks. `{ function f() {} }`
    // at module scope (outside any FnDecl) puts a FnDecl inside a
    // Stmt::Block, which the synthesized `main` body walks. Without
    // this pass, ssa_lower's lower_top_stmt catch-all panicked on
    // every such case (annexB function-statement hoisting tests).
    // Same shape as Pass 1 but the "parent" is the synthetic "__top"
    // namespace for mangling. Also handles If / While / DoWhile /
    // For / ForOf / ForIn / Try / Switch nested at module top.
    let parent = "__top".to_string();
    let mut top_renames: HashMap<String, String> = HashMap::new();
    let mut top_lifted: Vec<Stmt> = Vec::new();
    for stmt in top.iter_mut() {
        match stmt {
            Stmt::FnDecl { .. } => {} // handled by Pass 1
            other => {
                collect_nested_fns_in_stmt(
                    other,
                    &parent,
                    &mut top_renames,
                    &mut top_lifted,
                    &mut counter,
                );
            }
        }
    }
    if !top_renames.is_empty() {
        // Rewrite ident references in the entire top-level (excluding
        // the lifted decls themselves; those get rewritten below).
        for stmt in top.iter_mut() {
            rewrite_idents_in_stmt(ast, stmt, &top_renames);
        }
        for lf in top_lifted.iter_mut() {
            if let Stmt::FnDecl { body: lb, .. } = lf {
                rewrite_idents_in_body(ast, lb, &top_renames);
            }
        }
    }
    new_top.extend(top_lifted);
    top.extend(new_top);
    ast.stmts = top;
}

/// P3.4-followup-A — recursive helper that descends into a single stmt
/// looking for nested FnDecls (block/if/while/for/try/switch
/// children). When found, lift to `lifted` with mangled name and
/// drop from the original site (replaced with a no-op Block).
fn collect_nested_fns_in_stmt(
    stmt: &mut Stmt,
    parent_name: &str,
    renames: &mut HashMap<String, String>,
    lifted: &mut Vec<Stmt>,
    counter: &mut u32,
) {
    // Bare-FnDecl-as-statement: `if (cond) function f() {}` parses
    // the then-branch as Stmt::FnDecl directly (no enclosing Block).
    // Same lift-and-mangle as the Block case but in-place.
    if let Stmt::FnDecl { name, .. } = stmt {
        if !name.starts_with("__closure_")
            && !name.starts_with("__cm_")
            && !name.starts_with("__sm_")
            && !name.starts_with("__nested_")
        {
            let mangled = format!("__nested_{parent_name}_{name}_{counter}");
            *counter += 1;
            renames.insert(name.clone(), mangled.clone());
            // Take the FnDecl out of the slot, replace with empty
            // Block, push the renamed decl into `lifted`.
            let taken = std::mem::replace(stmt, Stmt::Block(Vec::new()));
            if let Stmt::FnDecl {
                type_params,
                params,
                return_type,
                body,
                is_generator,
                ..
            } = taken
            {
                lifted.push(Stmt::FnDecl {
                    name: mangled,
                    type_params,
                    params,
                    return_type,
                    body,
                    is_generator,
                });
            }
            return;
        }
    }
    match stmt {
        Stmt::Block(body) => {
            collect_nested_fns_to_lift(body, parent_name, renames, lifted, counter);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_nested_fns_in_stmt(then_branch, parent_name, renames, lifted, counter);
            if let Some(eb) = else_branch {
                collect_nested_fns_in_stmt(eb, parent_name, renames, lifted, counter);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_nested_fns_in_stmt(body, parent_name, renames, lifted, counter);
        }
        Stmt::For { body, .. } | Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => {
            collect_nested_fns_in_stmt(body, parent_name, renames, lifted, counter);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            collect_nested_fns_to_lift(body, parent_name, renames, lifted, counter);
            collect_nested_fns_to_lift(catch_body, parent_name, renames, lifted, counter);
            if let Some(fb) = finally_body {
                collect_nested_fns_to_lift(fb, parent_name, renames, lifted, counter);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for case in cases.iter_mut() {
                collect_nested_fns_to_lift(&mut case.body, parent_name, renames, lifted, counter);
            }
            if let Some(d) = default {
                collect_nested_fns_to_lift(d, parent_name, renames, lifted, counter);
            }
        }
        _ => {} // leaf stmts have no nested FnDecl children
    }
}

fn collect_nested_fns_to_lift(
    body: &mut Vec<Stmt>,
    parent_name: &str,
    renames: &mut HashMap<String, String>,
    lifted: &mut Vec<Stmt>,
    counter: &mut u32,
) {
    let drained = std::mem::take(body);
    let mut new_body: Vec<Stmt> = Vec::with_capacity(drained.len());
    for s in drained {
        match s {
            Stmt::FnDecl { ref name, .. }
                if !name.starts_with("__closure_")
                    && !name.starts_with("__cm_")
                    && !name.starts_with("__sm_")
                    && !name.starts_with("__nested_") =>
            {
                let mangled = format!("__nested_{parent_name}_{name}_{counter}");
                *counter += 1;
                renames.insert(name.clone(), mangled.clone());
                let new_decl = match s {
                    Stmt::FnDecl {
                        type_params,
                        params,
                        return_type,
                        body: fbody,
                        is_generator,
                        ..
                    } => Stmt::FnDecl {
                        name: mangled,
                        type_params,
                        params,
                        return_type,
                        body: fbody,
                        is_generator,
                    },
                    _ => unreachable!(),
                };
                lifted.push(new_decl);
                // Original site: drop entirely.
            }
            mut other => {
                collect_nested_fns_in_other(&mut other, parent_name, renames, lifted, counter);
                new_body.push(other);
            }
        }
    }
    *body = new_body;
}

fn collect_nested_fns_in_other(
    s: &mut Stmt,
    parent: &str,
    renames: &mut HashMap<String, String>,
    lifted: &mut Vec<Stmt>,
    counter: &mut u32,
) {
    match s {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_nested_fns_in_other(then_branch, parent, renames, lifted, counter);
            if let Some(eb) = else_branch.as_deref_mut() {
                collect_nested_fns_in_other(eb, parent, renames, lifted, counter);
            }
        }
        Stmt::Block(b) | Stmt::Multi(b) => {
            collect_nested_fns_to_lift(b, parent, renames, lifted, counter);
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_nested_fns_in_other(body, parent, renames, lifted, counter);
        }
        Stmt::For { body, .. } | Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => {
            collect_nested_fns_in_other(body, parent, renames, lifted, counter);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            collect_nested_fns_to_lift(body, parent, renames, lifted, counter);
            collect_nested_fns_to_lift(catch_body, parent, renames, lifted, counter);
            if let Some(fb) = finally_body {
                collect_nested_fns_to_lift(fb, parent, renames, lifted, counter);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for case in cases {
                collect_nested_fns_to_lift(&mut case.body, parent, renames, lifted, counter);
            }
            if let Some(dflt) = default {
                collect_nested_fns_to_lift(dflt, parent, renames, lifted, counter);
            }
        }
        // Don't descend into FnDecl — its body is its own scope
        // (handled at top level).
        _ => {}
    }
}

fn rewrite_idents_in_body(ast: &mut Ast, body: &mut Vec<Stmt>, renames: &HashMap<String, String>) {
    for s in body.iter_mut() {
        rewrite_idents_in_stmt(ast, s, renames);
    }
}

fn rewrite_idents_in_stmt(ast: &mut Ast, s: &mut Stmt, renames: &HashMap<String, String>) {
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

fn rewrite_idents_in_expr(ast: &mut Ast, eid: ExprId, renames: &HashMap<String, String>) {
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
