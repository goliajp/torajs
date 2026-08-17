//! Dead-copy sweep — tombstone the arena expressions the resolver
//! discarded, so no later pass can materialize them.
//!
//! Every lib module parses into the SAME arena as the entry
//! (`load_lib_section`), and only the statements the request wanted
//! survive into the splice; the rest are dropped at six different
//! sites (`walk_lib_stmt` / `inject_export` and friends), each an
//! implicit `Stmt` drop that leaves the statement's expression trees
//! stranded in the arena. Most whole-arena passes only REWRITE such
//! strays (harmless), but the lift family (`lift_arrow_fns` /
//! `hoist_gen_fn_exprs` / `hoist_nested_classes`) MATERIALIZES them
//! into top-level decls: a self-import (A→f→A) re-parses the entry's
//! own text as a lib, and the discarded copy's fn literals become
//! construction-less orphans — pass 2B's "has no capture types"
//! shelf (`ssa_lower_pass_2b`) and the assign-unknown-ident rejects
//! on dead arrow bodies both trace back here.
//!
//! The fix is the same hand `desugar_classes_generic_twin` plays for
//! its clone strays (`neutralize_clone`) and `heritage` plays for
//! consumed class parents (`tombstone_expr`), promoted from
//! per-channel cleanup to the module channel's single exit: after the
//! final splice, mark every expression reachable from `ast.stmts`
//! and overwrite the unreachable remainder with the `Expr::Null`
//! tombstone. Only ids at or past `entry_expr_len` (the arena length
//! before the first lib parse — `static_resolution::EntrySeed`) are
//! candidates: the entry's own arena keeps its deliberate orphans
//! (side-table-referenced nodes several desugars depend on), and it
//! never holds a discarded module copy.
//!
//! Failure mode is asymmetric by construction: a marker arm missed
//! here kills a live node LOUDLY (the conformance gate sees it),
//! while the pre-sweep status quo was a SILENT wrong program. Both
//! match statements below are exhaustive — a new `Expr` / `Stmt`
//! variant fails compilation until its children are accounted for.

use crate::ast::{Ast, Expr, ExprId, Param, StaticInit, Stmt};

/// Mark-and-tombstone entry point. Runs once, after
/// `assemble_and_splice` fixed the final statement list.
pub(super) fn sweep(ast: &mut Ast, entry_expr_len: usize) {
    if ast.exprs.len() <= entry_expr_len {
        return; // no lib ever parsed, nothing synthesized — no strays
    }
    let mut live = vec![false; ast.exprs.len()];
    for s in &ast.stmts {
        mark_stmt(ast, s, &mut live);
    }
    for (i, alive) in live.iter().enumerate().skip(entry_expr_len) {
        if !alive {
            ast.tombstone_expr(ExprId(i as u32));
        }
    }
}

fn mark_params(ast: &Ast, params: &[Param], live: &mut [bool]) {
    for p in params {
        if let Some(d) = p.default {
            mark_expr(ast, d, live);
        }
    }
}

fn mark_stmts(ast: &Ast, stmts: &[Stmt], live: &mut [bool]) {
    for s in stmts {
        mark_stmt(ast, s, live);
    }
}

fn mark_stmt(ast: &Ast, s: &Stmt, live: &mut [bool]) {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => mark_expr(ast, *e, live),
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => mark_expr(ast, *init, live),
        Stmt::YieldInto { value, .. } => mark_expr(ast, *value, live),
        Stmt::Return(o) => {
            if let Some(e) = o {
                mark_expr(ast, *e, live);
            }
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            mark_expr(ast, *cond, live);
            mark_stmt(ast, then_branch, live);
            if let Some(e) = else_branch {
                mark_stmt(ast, e, live);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            mark_expr(ast, *cond, live);
            mark_stmt(ast, body, live);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            mark_expr(ast, *scrutinee, live);
            for c in cases {
                mark_expr(ast, c.value, live);
                mark_stmts(ast, &c.body, live);
            }
            if let Some(d) = default {
                mark_stmts(ast, d, live);
            }
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                mark_stmt(ast, i, live);
            }
            for e in cond.iter().chain(step.iter()) {
                mark_expr(ast, *e, live);
            }
            mark_stmt(ast, body, live);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            mark_expr(ast, *parent, live);
            mark_expr(ast, *sep, live);
            mark_stmt(ast, body, live);
        }
        Stmt::ForOf {
            elem_expr,
            body,
            forin_obj,
            ..
        } => {
            mark_expr(ast, *elem_expr, live);
            if let Some(o) = forin_obj {
                mark_expr(ast, *o, live);
            }
            mark_stmt(ast, body, live);
        }
        Stmt::Labeled { body, .. } => mark_stmt(ast, body, live),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            mark_stmts(ast, body, live);
            mark_stmts(ast, catch_body, live);
            if let Some(f) = finally_body {
                mark_stmts(ast, f, live);
            }
        }
        Stmt::Block(v) | Stmt::Multi(v) => mark_stmts(ast, v, live),
        Stmt::FnDecl { params, body, .. } => {
            mark_params(ast, params, live);
            mark_stmts(ast, body, live);
        }
        Stmt::ClassDecl {
            parent,
            static_init,
            ctor,
            methods,
            static_methods,
            ..
        } => {
            if let Some(p) = parent {
                mark_expr(ast, *p, live);
            }
            for si in static_init {
                match si {
                    StaticInit::Field(f) => mark_expr(ast, f.init, live),
                    StaticInit::Block(b) => mark_stmts(ast, b, live),
                }
            }
            if let Some(c) = ctor {
                mark_params(ast, &c.params, live);
                mark_stmts(ast, &c.body, live);
            }
            for m in methods.iter().chain(static_methods.iter()) {
                mark_params(ast, &m.params, live);
                mark_stmts(ast, &m.body, live);
            }
        }
        Stmt::ExportDecl {
            inner,
            default_expr,
            ..
        } => {
            if let Some(s) = inner {
                mark_stmt(ast, s, live);
            }
            if let Some(e) = default_expr {
                mark_expr(ast, *e, live);
            }
        }
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::TypeDecl { .. } | Stmt::ImportDecl { .. } => {}
    }
}

fn mark_expr(ast: &Ast, eid: ExprId, live: &mut [bool]) {
    let slot = &mut live[eid.0 as usize];
    if *slot {
        return; // shared subtree (for-of elem_expr and friends)
    }
    *slot = true;
    match ast.get_expr(eid) {
        Expr::Ident(_)
        | Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::Null
        | Expr::Closure { .. }
        | Expr::This
        | Expr::NewTarget
        | Expr::Elision => {}
        Expr::Unary { expr, .. }
        | Expr::As { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr }
        | Expr::Member { obj: expr, .. }
        | Expr::OptChain { obj: expr, .. }
        | Expr::PostIncr { target: expr, .. } => mark_expr(ast, *expr, live),
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
        } => {
            mark_expr(ast, *left, live);
            mark_expr(ast, *right, live);
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            mark_expr(ast, *cond, live);
            mark_expr(ast, *then_branch, live);
            mark_expr(ast, *else_branch, live);
        }
        Expr::Call { callee, args }
        | Expr::OptCall { callee, args }
        | Expr::NewDynamic { callee, args } => {
            mark_expr(ast, *callee, live);
            for a in args {
                mark_expr(ast, *a, live);
            }
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                mark_expr(ast, *a, live);
            }
        }
        Expr::Array(items) => {
            for it in items {
                mark_expr(ast, *it, live);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                mark_expr(ast, *e, live);
            }
        }
        Expr::ArrowFn { params, body, .. } => {
            mark_params(ast, params, live);
            mark_stmts(ast, body, live);
        }
    }
}
