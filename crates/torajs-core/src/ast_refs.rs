//! K.3b — shared ground truth for promoting un-annotated top-level
//! `let`/`const` declarations to data-global slots.
//!
//! Both consumers MUST agree on which bindings promote, or programs
//! typecheck whose lowering aborts (checker registered, lower skipped)
//! — so the gate and the slot-shape inference live here, once:
//!
//!   - `check.rs` global pre-pass: registers the binding so named-fn
//!     bodies typecheck reads/writes against it.
//!   - `ssa_lower.rs` Pass 1.5: registers the actual data-global slot.
//!
//! Promotion is deliberately narrow. A binding only promotes when a
//! named fn body actually references it (named fns have no capture
//! machinery, so the slot is the only way they can see it) AND no
//! closure captures it (closure captures copy through `__env` from the
//! main-fn local — a promoted slot would split the binding into two
//! disagreeing homes). Everything else keeps the long-standing
//! main-fn-local behavior.

use crate::ast::{Ast, Expr, ExprId, Stmt};
use std::collections::HashSet;

// r290 file-size split — the slot-shape inference half; `pub use`
// keeps every `crate::ast_refs::*` consumer path unchanged.
mod shape;
mod shape_operator;
pub use shape::{
    GlobalSlotShape, infer_toplevel_slot_shape, lifted_closure_fn_canon, new_class_ann,
    objlit_literal_inlobj_ann,
};

pub struct ToplevelBindingRefs {
    /// Names referenced anywhere inside a named (non-lifted) fn body.
    /// Over-approximate: fn-local `let` shadows count too, which is
    /// safe — a needlessly promoted slot is shadowed by the local at
    /// lower time, while a missed name just keeps today's behavior.
    /// Param shadows do NOT count: a param name covers the whole fn
    /// body (fn scope), so an ident spelled like the param can never
    /// resolve to the top-level binding — excluding it is exact, and
    /// keeps a same-named top-level binding localizable.
    pub named_fn_refs: HashSet<String>,
    /// Names captured by any closure (flat scan over `Expr::Closure`
    /// capture lists, which `lift_arrow_fns` computed with proper
    /// scope tracking — nothing is missed).
    pub closure_captured: HashSet<String>,
}

pub fn toplevel_binding_refs(ast: &Ast) -> ToplevelBindingRefs {
    let mut named_fn_refs = HashSet::new();
    for stmt in &ast.stmts {
        if let Stmt::FnDecl { params, body, .. } = stmt {
            // lifted-closure FnDecls read outer bindings through the
            // hidden __env param — closure territory, not named-fn
            if params.first().is_some_and(|p| p.name == "__env") {
                continue;
            }
            let shadow: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
            for s in body {
                idents_in_stmt(ast, s, &shadow, &mut named_fn_refs);
            }
        }
    }
    let mut closure_captured = HashSet::new();
    for e in &ast.exprs {
        if let Expr::Closure { captures, .. } = e {
            closure_captured.extend(captures.iter().cloned());
        }
    }
    ToplevelBindingRefs {
        named_fn_refs,
        closure_captured,
    }
}

fn idents_in_stmt(ast: &Ast, s: &Stmt, shadow: &HashSet<String>, out: &mut HashSet<String>) {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => idents_in_expr(ast, *e, shadow, out),
        Stmt::LetDecl { init, .. } => idents_in_expr(ast, *init, shadow, out),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            idents_in_expr(ast, *cond, shadow, out);
            idents_in_stmt(ast, then_branch, shadow, out);
            if let Some(eb) = else_branch {
                idents_in_stmt(ast, eb, shadow, out);
            }
        }
        Stmt::Labeled { body, .. } => {
            idents_in_stmt(ast, body, shadow, out);
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            idents_in_expr(ast, *cond, shadow, out);
            idents_in_stmt(ast, body, shadow, out);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            idents_in_expr(ast, *scrutinee, shadow, out);
            for c in cases {
                idents_in_expr(ast, c.value, shadow, out);
                for cs in &c.body {
                    idents_in_stmt(ast, cs, shadow, out);
                }
            }
            if let Some(d) = default {
                for ds in d {
                    idents_in_stmt(ast, ds, shadow, out);
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
                idents_in_stmt(ast, i, shadow, out);
            }
            if let Some(c) = cond {
                idents_in_expr(ast, *c, shadow, out);
            }
            if let Some(st) = step {
                idents_in_expr(ast, *st, shadow, out);
            }
            idents_in_stmt(ast, body, shadow, out);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            idents_in_expr(ast, *parent, shadow, out);
            idents_in_expr(ast, *sep, shadow, out);
            idents_in_stmt(ast, body, shadow, out);
        }
        Stmt::ForOf {
            src_ident,
            elem_expr,
            body,
            ..
        } => {
            if !shadow.contains(src_ident) {
                out.insert(src_ident.clone());
            }
            idents_in_expr(ast, *elem_expr, shadow, out);
            idents_in_stmt(ast, body, shadow, out);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for bs in body {
                idents_in_stmt(ast, bs, shadow, out);
            }
            for cs in catch_body {
                idents_in_stmt(ast, cs, shadow, out);
            }
            if let Some(fb) = finally_body {
                for fs in fb {
                    idents_in_stmt(ast, fs, shadow, out);
                }
            }
        }
        Stmt::Block(v) | Stmt::Multi(v) => {
            for bs in v {
                idents_in_stmt(ast, bs, shadow, out);
            }
        }
        Stmt::FnDecl { params, body, .. } => {
            // nested fn (pre-desugar shape): its params shadow on top
            // of the enclosing fn's — an ident matching either set
            // can't resolve to a top-level binding from in here
            let inner: HashSet<String> = shadow
                .iter()
                .cloned()
                .chain(params.iter().map(|p| p.name.clone()))
                .collect();
            for bs in body {
                idents_in_stmt(ast, bs, &inner, out);
            }
        }
        Stmt::Return(maybe) => {
            if let Some(e) = maybe {
                idents_in_expr(ast, *e, shadow, out);
            }
        }
        Stmt::YieldInto { value, .. } => idents_in_expr(ast, *value, shadow, out),
        Stmt::ExportDecl {
            inner,
            default_expr,
            ..
        } => {
            if let Some(i) = inner {
                idents_in_stmt(ast, i, shadow, out);
            }
            if let Some(de) = default_expr {
                idents_in_expr(ast, *de, shadow, out);
            }
        }
        // No expressions inside (Break / Continue / TypeDecl /
        // ImportDecl), or never reaches the post-desugar consumers
        // (ClassDecl). Missing a reference is the safe direction:
        // the binding just keeps its main-fn-local behavior.
        _ => {}
    }
}

fn idents_in_expr(ast: &Ast, eid: ExprId, shadow: &HashSet<String>, out: &mut HashSet<String>) {
    match ast.get_expr(eid) {
        Expr::Ident(n) => {
            if !shadow.contains(n) {
                out.insert(n.clone());
            }
        }
        Expr::Call { callee, args } => {
            idents_in_expr(ast, *callee, shadow, out);
            for a in args {
                idents_in_expr(ast, *a, shadow, out);
            }
        }
        Expr::BinOp { left, right, .. } => {
            idents_in_expr(ast, *left, shadow, out);
            idents_in_expr(ast, *right, shadow, out);
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::As { expr, .. }
        | Expr::Delete { expr } => idents_in_expr(ast, *expr, shadow, out),
        Expr::InstanceOf { expr, rhs } => {
            idents_in_expr(ast, *expr, shadow, out);
            idents_in_expr(ast, *rhs, shadow, out);
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            idents_in_expr(ast, *obj, shadow, out)
        }
        Expr::OptIndex { obj, index } => {
            idents_in_expr(ast, *obj, shadow, out);
            idents_in_expr(ast, *index, shadow, out);
        }
        Expr::OptCall { callee, args } => {
            idents_in_expr(ast, *callee, shadow, out);
            for a in args {
                idents_in_expr(ast, *a, shadow, out);
            }
        }
        Expr::Assign { target, value } => {
            idents_in_expr(ast, *target, shadow, out);
            idents_in_expr(ast, *value, shadow, out);
        }
        Expr::Index { obj, index } => {
            idents_in_expr(ast, *obj, shadow, out);
            idents_in_expr(ast, *index, shadow, out);
        }
        Expr::Array(elems) => {
            for e in elems {
                idents_in_expr(ast, *e, shadow, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                idents_in_expr(ast, *e, shadow, out);
            }
        }
        Expr::ArrowFn { params, body, .. } => {
            // arrow params shadow on top of the enclosing fn's
            let inner: HashSet<String> = shadow
                .iter()
                .cloned()
                .chain(params.iter().map(|p| p.name.clone()))
                .collect();
            for s in body {
                idents_in_stmt(ast, s, &inner, out);
            }
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                idents_in_expr(ast, *a, shadow, out);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            idents_in_expr(ast, *cond, shadow, out);
            idents_in_expr(ast, *then_branch, shadow, out);
            idents_in_expr(ast, *else_branch, shadow, out);
        }
        Expr::Sequence { left, right }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        } => {
            idents_in_expr(ast, *left, shadow, out);
            idents_in_expr(ast, *right, shadow, out);
        }
        Expr::PostIncr { target, .. } => idents_in_expr(ast, *target, shadow, out),
        // r290 (closure-capture sweep cluster) — a closure minted
        // INSIDE a named fn body counts its captures as body
        // references: a named fn has no env machinery, so a capture
        // of a top-level binding can only resolve through the
        // globals table, and the binding must promote exactly like
        // a directly-read one ("closure capture not in scope"
        // otherwise). A capture that names a fn-local shadows
        // through the capture filter's locals-first order, so the
        // over-approximation stays safe (an extra reader-less slot).
        // Top-level-positioned closures are NOT walked here (their
        // lifted FnDecls carry `__env` and the loop skips them) and
        // keep the main-local capture-box machinery.
        Expr::Closure { captures, .. } => {
            out.extend(captures.iter().cloned());
        }
        // Literals / This / NewTarget / Uninit carry no idents.
        _ => {}
    }
}

// L3a-8's `fn_param_widens_to_f64` body-assignment scan retired in W1
// (ann-width RFC) — `num_width::f64_slots` is the module-wide width
// ground truth now, covering body assignments, call-site args, and
// slot-to-slot propagation from one fixpoint.
