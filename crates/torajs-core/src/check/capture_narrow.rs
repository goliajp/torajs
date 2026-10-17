//! The type a closure captures a null-narrowed binding at.
//!
//! A closure reads a captured binding whenever it is called, not where
//! it is created. So a closure first captures the `T | null` the narrow
//! replaced (`LocalInfo::null_declared`), as TS does for a binding
//! written after the closure. When its body does not check against
//! that type, the next run of the fixpoint keeps the narrow for this
//! capture (`Narrows::capture_kept`), unless a possibly-null store into
//! the binding can run while the closure exists without the walk
//! reaching it after the creation:
//!
//! - before the creation, inside a loop around it that does not
//!   declare the binding: the next iteration runs it after the closure
//!   exists;
//! - inside a closure, or in a function body for a top-level binding:
//!   it can run at any time (`LocalInfo::nulled_anytime`).
//!
//! Then the capture stays at `T | null` and the body's errors are the
//! program's. Otherwise it keeps the narrow and the binding is pinned:
//! a possibly-null store the walk reaches later, the closure's own
//! body included, is refused (`LocalInfo::pinned`). A store of a
//! non-null value never undoes the narrow, and a binding nothing ever
//! stores into keeps it with no trial.

use std::collections::HashSet;

use super::*;
use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::stmt_nested_lists::for_each_nested_list;

/// A closure's capture frame entries, and each capture's
/// `nulled_anytime` as the frame starts.
pub(crate) type CaptureFrame = (Vec<(String, LocalInfo)>, Vec<(String, bool)>);

/// What a closure body's capture frame says about the binding outside
/// it once the body is walked.
pub(crate) struct CaptureOut {
    name: String,
    nulled: bool,
    pinned: bool,
}

impl Checker {
    /// The type closure `eid` captures `name` (resolving to `info`)
    /// at, and whether it is the declared type on trial (see the module
    /// doc); keeping the narrow pins the binding.
    pub(crate) fn capture_ty(
        &mut self,
        ast: &Ast,
        eid: ExprId,
        name: &str,
        info: &LocalInfo,
    ) -> (Type, bool) {
        let Some(declared) = &info.null_declared else {
            return (info.ty.clone(), false);
        };
        if !info.mutable || matches!(info.ty, Type::Nullable(_)) || !any_write(ast, name) {
            return (info.ty.clone(), false);
        }
        if !self.narrows.capture_kept.contains(&(eid, name.to_string())) {
            return (declared.clone(), true);
        }
        if self.nulled_around(ast, eid, name, info) {
            return (declared.clone(), false);
        }
        if let Some(b) = self.scopes.iter_mut().rev().find_map(|s| s.get_mut(name)) {
            b.pinned = true;
        }
        (info.ty.clone(), false)
    }

    /// Open the walk of a closure body: the error count to measure it
    /// against.
    pub(crate) fn open_capture_trial(&mut self) -> usize {
        self.narrows.closure_errs.push(0);
        self.error_count()
    }

    /// Close the walk of closure `eid`'s body `fn_name`, whose captures
    /// `tried` were typed at their declared type, `failed` when the
    /// walk returned an error. Errors the body raised itself, not put
    /// down to a nested closure, ask the next run to keep the narrow
    /// of each tried capture that some read needs non-null; the errors
    /// stay with the enclosing body otherwise.
    pub(crate) fn close_capture_trial(
        &mut self,
        ast: &Ast,
        eid: ExprId,
        fn_name: &str,
        tried: &[String],
        before: usize,
        failed: bool,
    ) {
        let accounted = self
            .narrows
            .closure_errs
            .pop()
            .expect("close_capture_trial pairs with open_capture_trial");
        let grown = self.error_count().saturating_sub(before) + usize::from(failed);
        let mut kept = false;
        if grown > accounted && self.speculative_depth == 0 && !self.checking_spec_body {
            for n in tried {
                if !reads_tolerate_null(ast, fn_name, n, &mut HashSet::new()) {
                    self.narrows.capture_wanted.insert((eid, n.clone()));
                    kept = true;
                }
            }
        }
        if let Some(top) = self.narrows.closure_errs.last_mut() {
            *top += if kept { grown } else { accounted };
        }
    }

    fn error_count(&self) -> usize {
        self.errors
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    /// The member narrows a closure body could undo when called: those
    /// whose receiver or member the body, or a closure in it, writes.
    pub(crate) fn retire_member_narrows(&mut self, ast: &Ast, fn_name: &str) {
        if self.narrows.member.is_empty() {
            return;
        }
        let mut bodies = Vec::new();
        closure_bodies(ast, fn_name, &mut HashSet::new(), &mut bodies);
        self.narrows.member.retain(|(recv, field), _| {
            let hit = crate::check_assigns_to::member_hit(ast, recv, field);
            let base = |t: ExprId| match ast.get_expr(t) {
                Expr::Index { obj, .. }
                    if crate::check_assigns_to::member_path(ast, t).is_none() =>
                {
                    crate::check_assigns_to::member_path(ast, *obj)
                        .is_some_and(|bp| recv.starts_with(&format!("{bp}[")))
                }
                _ => false,
            };
            let either = |t: ExprId| hit(t) || base(t);
            !bodies
                .iter()
                .any(|s| crate::check_assigns_to::writes(ast, &[], s, &either))
        });
    }

    /// Can a possibly-null store into `name` run while closure `eid`
    /// exists, other than one the walk reaches after it? A store an
    /// enclosing closure made before `eid` runs again when that closure
    /// is called again, as a loop's next iteration would.
    pub(crate) fn nulled_around(
        &self,
        ast: &Ast,
        eid: ExprId,
        name: &str,
        info: &LocalInfo,
    ) -> bool {
        info.nulled_anytime
            || (info.captured && !info.null_stores.is_empty())
            || in_one_loop(ast, &ast.stmts, eid, name, &info.null_stores)
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
            captured: true,
            ..LocalInfo::plain(ty)
        }
    }

    /// The capture frame of closure `eid` (see [`Self::capture_frame_info`]),
    /// with each capture's `nulled_anytime` as the frame starts.
    pub(crate) fn capture_frame(
        &self,
        ast: &Ast,
        eid: ExprId,
        cap_tys: &[(String, Type)],
    ) -> CaptureFrame {
        let frame: Vec<(String, LocalInfo)> = cap_tys
            .iter()
            .map(|(n, t)| {
                let outer = self.lookup(n);
                let info = self.capture_frame_info(ast, eid, n, outer.as_ref(), t.clone());
                (n.clone(), info)
            })
            .collect();
        let inherited = frame
            .iter()
            .map(|(n, i)| (n.clone(), i.nulled_anytime))
            .collect();
        (frame, inherited)
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

/// Is there a loop in `stmts` holding both `eid` and one of `stores`,
/// and not the declaration of the binding `name` that `eid` sees?
fn in_one_loop(ast: &Ast, stmts: &[Stmt], eid: ExprId, name: &str, stores: &[ExprId]) -> bool {
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
            if ids.contains(&eid)
                && stores.iter().any(|v| ids.contains(v))
                && !declares_around(ast, s, eid, name)
            {
                return true;
            }
        }
        let mut hit = false;
        for_each_nested_list(s, &mut |l| {
            hit = hit || in_one_loop(ast, l, eid, name, stores)
        });
        hit
    })
}

/// Does `s`, which holds `eid`, declare a binding `name` whose scope
/// holds `eid`: in its head, or in a statement list on the way to it?
fn declares_around(ast: &Ast, s: &Stmt, eid: ExprId, name: &str) -> bool {
    let decl = |s: &Stmt| match s {
        Stmt::LetDecl { name: n, .. } => n == name,
        _ => false,
    };
    let head = match s {
        Stmt::For { init: Some(i), .. } => decl(i),
        Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => var_name == name,
        _ => false,
    };
    let holds = |s: &Stmt| {
        let mut ids = HashSet::new();
        collect_exprs(ast, s, &mut ids);
        ids.contains(&eid)
    };
    let mut hit = head;
    for_each_nested_list(s, &mut |l| {
        hit = hit
            || l.iter()
                .find(|s| holds(s))
                .is_some_and(|inner| l.iter().any(decl) || declares_around(ast, inner, eid, name));
    });
    hit
}

/// The body of lifted closure `fn_name` and of every closure in it.
fn closure_bodies<'a>(
    ast: &'a Ast,
    fn_name: &str,
    seen: &mut HashSet<String>,
    out: &mut Vec<&'a Stmt>,
) {
    if !seen.insert(fn_name.to_string()) {
        return;
    }
    let Some(body) = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl { name, body, .. } if name == fn_name => Some(body),
        _ => None,
    }) else {
        return;
    };
    let mut ids = HashSet::new();
    for s in body {
        collect_exprs(ast, s, &mut ids);
        out.push(s);
    }
    for e in ids {
        if let Expr::Closure { fn_name, .. } = ast.get_expr(e) {
            closure_bodies(ast, fn_name, seen, out);
        }
    }
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
