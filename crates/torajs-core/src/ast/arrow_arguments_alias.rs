//! Arrow-body `arguments` inheritance (ES §9.4.4 — an arrow has no
//! `arguments` binding of its own; a read resolves to the nearest
//! enclosing non-arrow function's). L3b ③, rotation 411 probe:
//! `function outer() { const f = () => arguments.length; return f() }`
//! answered 0 (bun: the outer argc) because `lift_arrow_fns` runs
//! BEFORE `desugar_arguments_object` — the arrow body, `arguments`
//! read included, moves into a fresh `__closure_N` FnDecl, and the
//! arguments pass then serves that fn's OWN (empty) argc.
//!
//! The fix is the textbook one (Babel's arrow transform does the
//! same): before the lift, alias the outer fn's `arguments` into an
//! ordinary local and point every arrow-interior read at it —
//!
//! ```text
//! function outer() {          function outer() {
//!   const f = () =>      →      let __arrowargs: any = arguments;
//!     arguments.length;         const f = () => __arrowargs.length;
//! }                           }
//! ```
//!
//! Everything downstream then works unchanged: the outer body's
//! `arguments` read makes `desugar_arguments_object` materialize the
//! object, and `lift_arrow_fns`' free-var walk captures the local
//! into the closure env. The alias name is deliberately ONE fixed
//! spelling: a nested non-arrow fn that needs its own alias declares
//! its own `__arrowargs`, and ordinary `let` shadowing IS the
//! nearest-enclosing-function rule.
//!
//! Scope walk: an `ArrowFn` body stays in the CURRENT fn's scope
//! (that is the point); a nested `Stmt::FnDecl` opens its own scope
//! and recurses independently. A miss in either direction is loud
//! downstream (the bare `arguments` ident stays unresolved), never
//! silently wrong.

use super::expr::Expr;
use super::stmt::Stmt;
use super::{Ast, ExprId};

pub fn alias_arrow_arguments(ast: &mut Ast) {
    let n = ast.stmts.len();
    for i in 0..n {
        if matches!(ast.stmts[i], Stmt::FnDecl { .. }) {
            let Stmt::FnDecl { body, .. } = &mut ast.stmts[i] else {
                unreachable!()
            };
            let mut b = std::mem::take(body);
            process_scope(ast, &mut b);
            let Stmt::FnDecl { body, .. } = &mut ast.stmts[i] else {
                unreachable!()
            };
            *body = b;
        }
    }
}

/// One non-arrow function scope: collect every `arguments` ident
/// sitting inside an arrow subtree of `body`, rewrite them to the
/// alias spelling, and inject the alias binding at the body head
/// when any were found.
fn process_scope(ast: &mut Ast, body: &mut Vec<Stmt>) {
    let mut hits: Vec<ExprId> = Vec::new();
    for s in body.iter_mut() {
        scan_stmt(ast, s, false, &mut hits);
    }
    if hits.is_empty() {
        return;
    }
    for eid in hits {
        ast.exprs[eid.0 as usize] = Expr::Ident("__arrowargs".to_string());
    }
    let init = ast.add_expr(Expr::Ident("arguments".to_string()));
    body.insert(
        0,
        Stmt::LetDecl {
            mutable: false,
            name: "__arrowargs".to_string(),
            type_ann: Some("any".to_string()),
            init,
            is_var: false,
        },
    );
}

/// Statement walk within one fn scope. `in_arrow` = the walk has
/// crossed at least one arrow boundary (reads here belong to the
/// enclosing fn). A nested `FnDecl` is its own scope — recurse
/// through [`process_scope`] and do not extend the current one.
fn scan_stmt(ast: &mut Ast, s: &mut Stmt, in_arrow: bool, hits: &mut Vec<ExprId>) {
    match s {
        Stmt::FnDecl { body, .. } => {
            let mut b = std::mem::take(body);
            process_scope(ast, &mut b);
            *body = b;
        }
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => scan_expr(ast, *e, in_arrow, hits),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                scan_expr(ast, *e, in_arrow, hits);
            }
        }
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => {
            scan_expr(ast, *init, in_arrow, hits);
        }
        Stmt::YieldInto { value, .. } => scan_expr(ast, *value, in_arrow, hits),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            scan_expr(ast, *cond, in_arrow, hits);
            scan_stmt(ast, then_branch, in_arrow, hits);
            if let Some(e) = else_branch {
                scan_stmt(ast, e, in_arrow, hits);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
            scan_expr(ast, *cond, in_arrow, hits);
            scan_stmt(ast, body, in_arrow, hits);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            scan_expr(ast, *scrutinee, in_arrow, hits);
            for c in cases.iter_mut() {
                scan_expr(ast, c.value, in_arrow, hits);
                for cs in c.body.iter_mut() {
                    scan_stmt(ast, cs, in_arrow, hits);
                }
            }
            if let Some(d) = default {
                for ds in d.iter_mut() {
                    scan_stmt(ast, ds, in_arrow, hits);
                }
            }
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(s0) = init {
                scan_stmt(ast, s0, in_arrow, hits);
            }
            if let Some(c) = cond {
                scan_expr(ast, *c, in_arrow, hits);
            }
            if let Some(st) = step {
                scan_expr(ast, *st, in_arrow, hits);
            }
            scan_stmt(ast, body, in_arrow, hits);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            scan_expr(ast, *parent, in_arrow, hits);
            scan_expr(ast, *sep, in_arrow, hits);
            scan_stmt(ast, body, in_arrow, hits);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            scan_expr(ast, *elem_expr, in_arrow, hits);
            scan_stmt(ast, body, in_arrow, hits);
        }
        Stmt::Labeled { body, .. } => scan_stmt(ast, body, in_arrow, hits),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for bs in body.iter_mut() {
                scan_stmt(ast, bs, in_arrow, hits);
            }
            for cs in catch_body.iter_mut() {
                scan_stmt(ast, cs, in_arrow, hits);
            }
            if let Some(fb) = finally_body {
                for fs in fb.iter_mut() {
                    scan_stmt(ast, fs, in_arrow, hits);
                }
            }
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for bs in stmts.iter_mut() {
                scan_stmt(ast, bs, in_arrow, hits);
            }
        }
        _ => {}
    }
}

/// Expression walk. Recursion collects child ids first (the arena is
/// borrowed immutably per node), then descends; an `ArrowFn` body is
/// taken out of the node, walked with `in_arrow = true`, and put
/// back.
fn scan_expr(ast: &mut Ast, eid: ExprId, in_arrow: bool, hits: &mut Vec<ExprId>) {
    // Arrow boundary — the body walks in this SAME fn scope, flagged.
    if matches!(ast.get_expr(eid), Expr::ArrowFn { .. }) {
        let Expr::ArrowFn { body, .. } = &mut ast.exprs[eid.0 as usize] else {
            unreachable!()
        };
        let mut b = std::mem::take(body);
        for s in b.iter_mut() {
            scan_stmt(ast, s, true, hits);
        }
        let Expr::ArrowFn { body, .. } = &mut ast.exprs[eid.0 as usize] else {
            unreachable!()
        };
        *body = b;
        return;
    }
    if in_arrow && matches!(ast.get_expr(eid), Expr::Ident(n) if n == "arguments") {
        hits.push(eid);
        return;
    }
    let children: Vec<ExprId> = match ast.get_expr(eid) {
        Expr::BinOp { left, right, .. } => vec![*left, *right],
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::Delete { expr, .. }
        | Expr::As { expr, .. } => vec![*expr],
        Expr::PostIncr { target, .. } => vec![*target],
        Expr::Member { obj, .. } => vec![*obj],
        Expr::Index { obj, index } => vec![*obj, *index],
        Expr::Call { callee, args } => {
            let mut v = vec![*callee];
            v.extend(args.iter().copied());
            v
        }
        Expr::Assign { target, value } => vec![*target, *value],
        Expr::Array(items) => items.clone(),
        Expr::ObjectLit { fields } => fields.iter().map(|(_, e)| *e).collect(),
        Expr::New { args, .. } | Expr::Super { args } => args.clone(),
        Expr::NewDynamic { callee, args } => {
            let mut v = vec![*callee];
            v.extend(args.iter().copied());
            v
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => vec![*cond, *then_branch, *else_branch],
        Expr::Sequence { left, right } => vec![*left, *right],
        Expr::Nullish { lhs, rhs } => vec![*lhs, *rhs],
        Expr::InstanceOf { expr, rhs } => vec![*expr, *rhs],
        Expr::OptChain { obj, .. } => vec![*obj],
        Expr::OptIndex { obj, index } => vec![*obj, *index],
        Expr::OptCall { callee, args } => {
            let mut v = vec![*callee];
            v.extend(args.iter().copied());
            v
        }
        _ => Vec::new(),
    };
    for c in children {
        scan_expr(ast, c, in_arrow, hits);
    }
}
