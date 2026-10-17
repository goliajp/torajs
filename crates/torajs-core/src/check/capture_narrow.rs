//! The type a closure captures a null-narrowed binding at.
//!
//! A closure reads a captured binding whenever it is called, not where
//! it is created, so the narrow in force at creation holds for it only
//! if no possibly-null store into the binding can run between the
//! creation and a call. Such a store can be
//!
//! - after the creation in the walk: the closure keeps the narrow and
//!   the binding is pinned, so that store is refused when it is
//!   reached (`LocalInfo::pinned`);
//! - before the creation, inside a loop around it: the next iteration
//!   runs it after the closure exists;
//! - inside a closure, or in a function body for a top-level binding:
//!   it can run at any time (`LocalInfo::nulled_anytime`).
//!
//! In the last two cases the closure captures the `T | null` the
//! narrow replaced (`LocalInfo::null_declared`), and so it does when
//! its body reads the binding only where null is welcome (compared
//! with null, `typeof`, `??`, `?.`) and some write to it exists: such
//! a closure does not need the narrow, and pinning would refuse a
//! later null store for nothing. A store of a non-null value never
//! undoes the narrow.

use std::collections::HashSet;

use super::*;
use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::stmt_nested_lists::for_each_nested_list;

/// What a closure body's capture frame says about the binding outside
/// it once the body is walked.
pub(crate) struct CaptureOut {
    name: String,
    nulled: bool,
    pinned: bool,
}

impl Checker {
    /// The type closure `eid` captures `name` (resolving to `info`)
    /// at; keeping the narrow pins the binding.
    pub(crate) fn capture_ty(
        &mut self,
        ast: &Ast,
        eid: ExprId,
        name: &str,
        info: &LocalInfo,
    ) -> Type {
        let Some(declared) = &info.null_declared else {
            return info.ty.clone();
        };
        if !info.mutable || matches!(info.ty, Type::Nullable(_)) {
            return info.ty.clone();
        }
        // a closure that reads the binding only where null is welcome
        // does not need the narrow, and then a write need not be refused
        let tolerant = || match ast.get_expr(eid) {
            Expr::Closure { fn_name, .. } => {
                any_write(ast, name) && reads_tolerate_null(ast, fn_name, name, &mut HashSet::new())
            }
            _ => false,
        };
        if self.nulled_around(ast, eid, name, info) || tolerant() {
            return declared.clone();
        }
        if let Some(b) = self.scopes.iter_mut().rev().find_map(|s| s.get_mut(name)) {
            b.pinned = true;
        }
        info.ty.clone()
    }

    /// Can a possibly-null store into `name` run while closure `eid`
    /// exists, other than one the walk reaches after it?
    pub(crate) fn nulled_around(
        &self,
        ast: &Ast,
        eid: ExprId,
        name: &str,
        info: &LocalInfo,
    ) -> bool {
        info.nulled_anytime
            || in_one_loop(ast, &ast.stmts, eid, &info.null_stores)
            || (self.is_main_toplevel(name) && self.written_in_fn_body(ast, name))
    }

    /// The capture frame entry of `name` at `ty` in the body of closure
    /// `eid`: it inherits what the binding outside knows, its narrow's
    /// declared type, its pin, and whether a store can run at any time.
    pub(crate) fn capture_frame_info(
        &self,
        ast: &Ast,
        eid: ExprId,
        name: &str,
        outer: Option<&LocalInfo>,
        ty: Type,
    ) -> LocalInfo {
        let Some(outer) = outer else {
            return LocalInfo::plain(ty);
        };
        LocalInfo {
            fn_slot: outer.fn_slot.clone(),
            union: outer.union.clone(),
            held_any: outer.held_any,
            ann: outer.ann.clone(),
            decl: outer.decl.clone(),
            null_declared: outer.null_declared.clone(),
            nulled_anytime: self.nulled_around(ast, eid, name, outer),
            pinned: outer.pinned,
            ..LocalInfo::plain(ty)
        }
    }

    /// After the body: which captures it stored a possibly-null value
    /// into, or pinned.
    /// `inherited` names each capture with the `nulled_anytime` its
    /// entry started with.
    pub(crate) fn capture_frame_out(
        &self,
        frame: usize,
        inherited: &[(String, bool)],
    ) -> Vec<CaptureOut> {
        inherited
            .iter()
            .filter_map(|(name, nulled_before)| {
                let info = self.scopes.get(frame)?.get(name)?;
                Some(CaptureOut {
                    name: name.clone(),
                    nulled: !info.null_stores.is_empty() || (info.nulled_anytime && !nulled_before),
                    pinned: info.pinned,
                })
            })
            .collect()
    }

    pub(crate) fn apply_capture_out(&mut self, out: Vec<CaptureOut>) {
        for o in out {
            if let Some(b) = self
                .scopes
                .iter_mut()
                .rev()
                .find_map(|s| s.get_mut(&o.name))
            {
                b.nulled_anytime |= o.nulled;
                b.pinned |= o.pinned;
            }
        }
    }

    /// A store of `value_ty` into the binding `name`: a possibly-null
    /// one is recorded, and refused when a closure reads the binding at
    /// its non-null narrow.
    pub(crate) fn note_store(
        &mut self,
        name: &str,
        value: ExprId,
        value_ty: &Type,
    ) -> Result<(), String> {
        if !matches!(
            value_ty,
            Type::Null | Type::Undefined | Type::Nullable(_) | Type::Void | Type::Any
        ) {
            return Ok(());
        }
        let Some(b) = self.scopes.iter_mut().rev().find_map(|s| s.get_mut(name)) else {
            return Ok(());
        };
        if b.pinned {
            return Err(format!(
                "a closure created above reads `{name}` as non-null, \
                 and this store may leave it null"
            ));
        }
        b.null_stores.push(value);
        Ok(())
    }

    /// Is `name` the binding of the top-level code's own frame?
    fn is_main_toplevel(&self, name: &str) -> bool {
        self.expected_return.is_none()
            && self.scopes.first().is_some_and(|s| s.contains_key(name))
            && self.scopes.iter().filter(|s| s.contains_key(name)).count() == 1
    }

    /// Does a function or class body anywhere in the program store into
    /// a binding spelled `name`? Those reach a top-level binding
    /// without capturing it.
    fn written_in_fn_body(&self, ast: &Ast, name: &str) -> bool {
        self.narrows
            .fn_body_writes
            .get_or_init(|| fn_body_writes(ast))
            .contains(name)
    }
}

/// Does any expression of the program store into a binding spelled
/// `name`?
fn any_write(ast: &Ast, name: &str) -> bool {
    ast.exprs.iter().any(|e| match e {
        Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => {
            matches!(ast.get_expr(*target), Expr::Ident(n) if n == name)
        }
        _ => false,
    })
}

/// Does the lifted body `fn_name` (and every closure in it capturing
/// `name`) read `name` only where a null is welcome: compared with
/// null or undefined, under `typeof`, on the left of `??`, or as the
/// base of an optional chain?
fn reads_tolerate_null(ast: &Ast, fn_name: &str, name: &str, seen: &mut HashSet<String>) -> bool {
    if !seen.insert(fn_name.to_string()) {
        return true;
    }
    let Some(body) = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl { name: n, body, .. } if n == fn_name => Some(body),
        _ => None,
    }) else {
        return false;
    };
    let mut exprs = Vec::new();
    for s in body {
        visit_stmt_exprs(s, &mut |e| exprs.push(e));
    }
    exprs
        .into_iter()
        .all(|e| expr_tolerates_null(ast, e, name, seen))
}

fn expr_tolerates_null(ast: &Ast, e: ExprId, name: &str, seen: &mut HashSet<String>) -> bool {
    let is_name = |x: ExprId| matches!(ast.get_expr(x), Expr::Ident(n) if n == name);
    let nullish = |x: ExprId| match ast.get_expr(x) {
        Expr::Null => true,
        Expr::Ident(n) => n == "undefined",
        _ => false,
    };
    let skip = match ast.get_expr(e) {
        Expr::Ident(n) => return n != name,
        Expr::Closure { fn_name, captures } if captures.iter().any(|c| c == name) => {
            return reads_tolerate_null(ast, fn_name, name, seen);
        }
        Expr::BinOp {
            op: BinOp::Eq | BinOp::Neq | BinOp::LooseEq | BinOp::LooseNeq,
            left,
            right,
        } if (is_name(*left) && nullish(*right)) || (is_name(*right) && nullish(*left)) => {
            return true;
        }
        Expr::Assign { target, .. } if is_name(*target) => Some(*target),
        Expr::TypeOf { expr } if is_name(*expr) => Some(*expr),
        Expr::Nullish { lhs, .. } if is_name(*lhs) => Some(*lhs),
        Expr::OptChain { obj, .. } | Expr::OptIndex { obj, .. } if is_name(*obj) => Some(*obj),
        Expr::OptCall { callee, .. } if is_name(*callee) => Some(*callee),
        _ => None,
    };
    let mut ok = true;
    visit_expr_children(ast, e, &mut |c| {
        ok = ok && (Some(c) == skip || expr_tolerates_null(ast, c, name, seen));
    });
    ok
}

/// The names stored into outside the top-level code's own statements.
fn fn_body_writes(ast: &Ast) -> HashSet<String> {
    let mut main = HashSet::new();
    for s in &ast.stmts {
        collect_exprs(ast, s, &mut main);
    }
    ast.exprs
        .iter()
        .enumerate()
        .filter(|(i, _)| !main.contains(&ExprId(*i as u32)))
        .filter_map(|(_, e)| match e {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => {
                match ast.get_expr(*target) {
                    Expr::Ident(n) => Some(n.clone()),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect()
}

/// Is there a loop in `stmts` holding both `eid` and one of `stores`?
fn in_one_loop(ast: &Ast, stmts: &[Stmt], eid: ExprId, stores: &[ExprId]) -> bool {
    if stores.is_empty() {
        return false;
    }
    stmts.iter().any(|s| {
        if matches!(
            s,
            Stmt::While { .. }
                | Stmt::DoWhile { .. }
                | Stmt::For { .. }
                | Stmt::ForOf { .. }
                | Stmt::ForOfSplitIter { .. }
        ) {
            let mut ids = HashSet::new();
            collect_exprs(ast, s, &mut ids);
            if ids.contains(&eid) && stores.iter().any(|v| ids.contains(v)) {
                return true;
            }
        }
        let mut hit = false;
        for_each_nested_list(s, &mut |l| hit = hit || in_one_loop(ast, l, eid, stores));
        hit
    })
}

/// Every expression `s` evaluates in its own frame.
fn collect_exprs(ast: &Ast, s: &Stmt, out: &mut HashSet<ExprId>) {
    fn expr(ast: &Ast, e: ExprId, out: &mut HashSet<ExprId>) {
        if out.insert(e) {
            visit_expr_children(ast, e, &mut |c| expr(ast, c, out));
        }
    }
    visit_stmt_exprs(s, &mut |e| expr(ast, e, out));
}
