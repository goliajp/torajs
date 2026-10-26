//! Which `let m = re.exec(s)` results never need the match array —
//! the census the exec-record lane consults.
//!
//! A hit builds an array, copies every group into a fresh string and
//! attaches the `index` / `input` / `groups` bag; a program that then
//! only null-tests the result, reads `m.index`, indexes it by a
//! constant and measures the group, pays for all of that and for its
//! release to learn a few numbers. None of it is observable in these
//! shapes, so the lowering holds the result as its spans instead
//! (scalar replacement of an aggregate that does not escape):
//! `__torajs_regex_exec_span` writes group starts and ends into a
//! stack record, with every lastIndex effect `exec` has.
//!
//! A use of `m` is the lane's when it is
//!
//! - `m === null` / `!==` / `==` / `!=` against a bare `null` literal;
//! - `!m`, a bare `m` as an `if` / loop / `?:` condition;
//! - `m.index` or `m.input` read as a value;
//! - `m[k]` with a constant integer `k` read as a value;
//! - `m[k].length` read as a value;
//! - `m = re.exec(t)` — re-assignment from the same regex binding.
//!
//! Anything else — `m.length`, `m.groups`, `m[i]` with a computed
//! index, `m` handed anywhere, `m ?? x`, `m?.x`, `typeof m`, a write
//! through `m` — keeps the array. The regex is one identifier, the
//! same for the declaration and every re-assignment, and nothing in
//! the binding's scope assigns or re-declares it: the lowering then
//! knows one receiver type for every call that fills the record.
//!
//! Whether that receiver is a RegExp is not known here; the checker
//! and the lowering both take the lane only when it is, so an entry
//! for any other call is inert. Keyed by the `.exec` member ExprId of
//! the declaration and of each re-assignment, each mapped to the
//! number of group slots the record needs (the largest constant index
//! read, plus one). The walk and the scope rules are the shared ones
//! in [`super::scalar_census`].

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::scalar_census::{Census, Lane, census_bindings, expr_uses_ok, is_x};
use super::{Ast, BinOp, Expr, ExprId, Stmt, UnaryOp};

/// The largest constant index the record holds a span for; a read
/// past it keeps the array.
const MAX_INDEX: u32 = 63;

/// The `.exec` member ExprIds whose result the lane may hold as spans,
/// each with the number of group slots its record needs.
pub(super) fn regex_exec_record_callees(ast: &Ast, census: &Census) -> HashMap<ExprId, u32> {
    let lane = ExecLane {
        slots: RefCell::new(HashMap::new()),
    };
    census_bindings(ast, &lane, census);
    lane.slots.into_inner()
}

struct ExecLane {
    slots: RefCell<HashMap<ExprId, u32>>,
}

/// `(callee, receiver name, argument)` of `re.exec(arg)`.
fn exec_call(ast: &Ast, e: ExprId) -> Option<(ExprId, &str, ExprId)> {
    let Expr::Call { callee, args } = ast.get_expr(e) else {
        return None;
    };
    let Expr::Member { obj, name } = ast.get_expr(*callee) else {
        return None;
    };
    let Expr::Ident(re) = ast.get_expr(*obj) else {
        return None;
    };
    match args.as_slice() {
        [arg] if name == "exec" && !matches!(ast.get_expr(*arg), Expr::Spread { .. }) => {
            Some((*callee, re, *arg))
        }
        _ => None,
    }
}

/// `x[k]` with a constant integer `k` the record can hold: `k`.
fn const_index_of_x(ast: &Ast, e: ExprId, x: &str) -> Option<u32> {
    let Expr::Index { obj, index } = ast.get_expr(e) else {
        return None;
    };
    if !is_x(ast, *obj, x) {
        return None;
    }
    let Expr::Number(n) = ast.get_expr(*index) else {
        return None;
    };
    (*n >= 0.0 && *n <= f64::from(MAX_INDEX) && n.fract() == 0.0).then_some(*n as u32)
}

/// `x` appears anywhere under `e`.
fn mentions(ast: &Ast, e: ExprId, x: &str) -> bool {
    is_x(ast, e, x)
        || expr_children(ast, e)
            .into_iter()
            .any(|c| mentions(ast, c, x))
}

impl Lane for ExecLane {
    fn produced(&self, ast: &Ast, init: ExprId) -> Option<(ExprId, ExprId)> {
        exec_call(ast, init).map(|(callee, _, _)| (init, callee))
    }

    fn leaf(&self, ast: &Ast, e: ExprId, x: &str, census: &Census) -> Option<bool> {
        let is_null = |id: ExprId| matches!(ast.get_expr(id), Expr::Null);
        let read = !census.not_read.contains(&e);
        match ast.get_expr(e) {
            Expr::BinOp {
                op: BinOp::Eq | BinOp::Neq | BinOp::LooseEq | BinOp::LooseNeq,
                left,
                right,
            } if (is_x(ast, *left, x) && is_null(*right))
                || (is_null(*left) && is_x(ast, *right, x)) =>
            {
                Some(true)
            }
            Expr::Unary {
                op: UnaryOp::Not,
                expr,
            } if is_x(ast, *expr, x) => Some(true),
            Expr::Member { obj, name }
                if read && is_x(ast, *obj, x) && (name == "index" || name == "input") =>
            {
                Some(true)
            }
            Expr::Member { obj, name }
                if read && name == "length" && const_index_of_x(ast, *obj, x).is_some() =>
            {
                Some(true)
            }
            Expr::Index { .. } if read && const_index_of_x(ast, e, x).is_some() => Some(true),
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } if is_x(ast, *cond, x) => Some(
                expr_uses_ok(ast, *then_branch, x, self, census)
                    && expr_uses_ok(ast, *else_branch, x, self, census),
            ),
            Expr::Assign { target, value } if is_x(ast, *target, x) => Some(matches!(
                exec_call(ast, *value),
                Some((_, _, arg)) if !mentions(ast, arg, x)
            )),
            _ => None,
        }
    }

    fn bare_condition_ok(&self) -> bool {
        true
    }

    fn accept(
        &self,
        ast: &Ast,
        name: &str,
        init: ExprId,
        call: ExprId,
        key: ExprId,
        window: &[&Stmt],
        census: &Census,
        found: &mut HashSet<ExprId>,
    ) {
        let Some((_, re, _)) = exec_call(ast, init) else {
            return;
        };
        let mut scan = WindowScan {
            x: name,
            re,
            rebinds: Vec::new(),
            max_index: 0,
        };
        if !window.iter().all(|s| scan.stmt(ast, s))
            || !census.single_use(call, key)
            || !scan.rebinds.iter().all(|&(c, k)| census.single_use(c, k))
        {
            return;
        }
        let n_slots = scan.max_index + 1;
        let mut slots = self.slots.borrow_mut();
        slots.insert(key, n_slots);
        for (_, k) in scan.rebinds {
            slots.insert(k, n_slots);
        }
        found.insert(key);
    }
}

/// The facts the record needs from the binding's window: every
/// re-assignment's call, the largest constant index read, and that
/// the regex binding is neither assigned nor re-declared in it.
struct WindowScan<'a> {
    x: &'a str,
    re: &'a str,
    rebinds: Vec<(ExprId, ExprId)>,
    max_index: u32,
}

impl WindowScan<'_> {
    fn stmt(&mut self, ast: &Ast, s: &Stmt) -> bool {
        let redeclares_re = match s {
            Stmt::LetDecl { name, .. } | Stmt::UsingDecl { name, .. } => name == self.re,
            Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => {
                var_name == self.re
            }
            Stmt::Try {
                catch_param: Some(p),
                ..
            } => p == self.re,
            _ => false,
        };
        !redeclares_re
            && stmt_exprs(s).into_iter().all(|e| self.expr(ast, e))
            && stmt_children_ref(s).into_iter().all(|c| self.stmt(ast, c))
    }

    fn expr(&mut self, ast: &Ast, e: ExprId) -> bool {
        match ast.get_expr(e) {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. }
                if is_x(ast, *target, self.re) =>
            {
                return false;
            }
            Expr::Assign { target, value } if is_x(ast, *target, self.x) => {
                match exec_call(ast, *value) {
                    Some((callee, r, _)) if r == self.re => self.rebinds.push((*value, callee)),
                    _ => return false,
                }
            }
            _ => {}
        }
        if let Some(k) = const_index_of_x(ast, e, self.x) {
            self.max_index = self.max_index.max(k);
        }
        expr_children(ast, e).into_iter().all(|c| self.expr(ast, c))
    }
}
