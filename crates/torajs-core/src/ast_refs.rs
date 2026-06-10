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

use crate::ast::{Ast, Expr, ExprId, Stmt, UnaryOp};
use std::collections::HashSet;

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

/// Slot type for an un-annotated top-level declaration, recovered from
/// initializer shapes whose runtime type is statically certain. Number
/// width must match what lowering actually produces for the same
/// expression — a guess in the wrong direction stores f64 bits in an
/// i64 slot (garbage on every read), so anything uncertain returns
/// None and the binding stays a main-fn local.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalSlotShape {
    I64,
    F64,
    Str,
    Bool,
}

pub fn infer_toplevel_slot_shape(ast: &Ast, init: ExprId) -> Option<GlobalSlotShape> {
    match ast.get_expr(init) {
        // Mirrors infer_arg_width: genuinely fractional or past i64
        // range must be f64; integral literals take the i64 default.
        Expr::Number(n) => Some(if n.fract() != 0.0 || n.abs() >= 9.223372036854776e18 {
            GlobalSlotShape::F64
        } else {
            GlobalSlotShape::I64
        }),
        Expr::String(_) => Some(GlobalSlotShape::Str),
        Expr::Bool(_) => Some(GlobalSlotShape::Bool),
        Expr::Ident(n) if n == "NaN" || n == "Infinity" => Some(GlobalSlotShape::F64),
        Expr::Unary {
            op: UnaryOp::Neg,
            expr,
        } => infer_toplevel_slot_shape(ast, *expr),
        // Call to a top-level named fn: the return annotation is the
        // same ground truth lowering uses for the callee's ret slot.
        Expr::Call { callee, .. } => {
            let Expr::Ident(fname) = ast.get_expr(*callee) else {
                return None;
            };
            ast.stmts.iter().find_map(|s| match s {
                Stmt::FnDecl {
                    name,
                    return_type,
                    params,
                    ..
                } if name == fname && params.first().is_none_or(|p| p.name != "__env") => {
                    match return_type.as_deref() {
                        Some("number") | Some("i64") => Some(GlobalSlotShape::I64),
                        Some("f64") => Some(GlobalSlotShape::F64),
                        Some("string") => Some(GlobalSlotShape::Str),
                        Some("boolean") => Some(GlobalSlotShape::Bool),
                        _ => None,
                    }
                }
                _ => None,
            })
        }
        _ => None,
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
        | Expr::InstanceOf { expr, .. }
        | Expr::As { expr, .. } => idents_in_expr(ast, *expr, shadow, out),
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            idents_in_expr(ast, *obj, shadow, out)
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
        // Closure captures are collected by the flat Expr::Closure
        // scan in toplevel_binding_refs; the lifted body is a
        // top-level FnDecl that the named-fn loop already skips.
        // Literals / This / NewTarget / Uninit carry no idents.
        _ => {}
    }
}
