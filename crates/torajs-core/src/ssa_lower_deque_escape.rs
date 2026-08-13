//! 11-A1 — fn-local escape analysis for Array<T> bindings.
//!
//! Walks the fn body's AST and collects binding names of Arrays
//! that **may** be shifted / unshifted / escape across fn-call
//! boundary / get stored into another heap object. Conservative
//! by design: any pattern that can't be statically proven safe
//! gets added (false-positive = miss a fast-path opportunity,
//! false-negative = silent-wrong wrong-offset read; only the
//! former is acceptable).
//!
//! Returns the set of "deque-unsafe" binding names. The
//! complementary "safe to skip head load at runtime-idx index
//! read" decision is made at the emit site: `name ∈ deque_arrs`
//! ⇒ slow path (head-aware emit), `name ∉ deque_arrs` ⇒ 11-A1
//! fast-path (`ARR_DATA_OFF + idx*8` directly, no head load).
//!
//! First-version escape patterns covered:
//! - `xs.shift()` / `xs.unshift(v)` — direct head mutation
//! - any fn-call with `Ident(xs)` as a bare arg — callee may shift
//! - store-into-heap: `obj.f = xs` / `arr[i] = xs` / array-literal
//!   element `[a, xs, b]` / object-literal field `{f: xs}` / spread
//!   `...xs` — escape through memory
//! - return `xs` — escape through caller
//! - alias: `let y = xs` or `y = xs` — both bindings get marked
//!   (per-binding precision not tracked yet); value-transparent
//!   wrappers are looked through (`cond ? xs : zs` / `xs ?? zs` /
//!   `(e, xs)` / `xs as T` / nested assign), so aliased sources
//!   behind them are marked too — see `collect_value_flow_idents`
//! - `for (...of xs)` source — runtime iter path unaudited
//! - closure capture of `xs` — lifted body may shift
//!
//! Lives in its own file (not collocated with the other AST
//! visitors in `ssa_lower.rs`) so `ssa_lower.rs` stays on the
//! "known-debt only-shrinks" trajectory; future sub-steps of the
//! `ssa_lower` carve-out plan move the other visitors here too.

use std::collections::HashSet;

use crate::ast::{Ast, Expr, ExprId, Stmt};

/// Collect binding names that can be the *value* of this expression,
/// looking through value-transparent wrappers: ternary arms, nullish
/// arms, sequence results, `as` casts and nested assigns. Escape
/// sites must use this instead of a bare-`Ident` match — an array
/// aliased behind `cond ? xs : ys` escapes exactly like a bare `xs`
/// (missing it is a false-negative = wrong-offset fast-path read).
/// Shared with the 11-A2-a obj-escape visitor — same hole there is
/// a stack-use-after-free.
pub(crate) fn collect_value_flow_idents(ast: &Ast, eid: ExprId, out: &mut HashSet<String>) {
    match ast.get_expr(eid) {
        Expr::Ident(n) => {
            out.insert(n.clone());
        }
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => {
            collect_value_flow_idents(ast, *then_branch, out);
            collect_value_flow_idents(ast, *else_branch, out);
        }
        Expr::Nullish { lhs, rhs } => {
            collect_value_flow_idents(ast, *lhs, out);
            collect_value_flow_idents(ast, *rhs, out);
        }
        Expr::Sequence { right, .. } => {
            collect_value_flow_idents(ast, *right, out);
        }
        Expr::As { expr, .. } => {
            collect_value_flow_idents(ast, *expr, out);
        }
        Expr::Assign { value, .. } => {
            collect_value_flow_idents(ast, *value, out);
        }
        _ => {}
    }
}

pub(crate) fn collect_deque_arr_names_in_stmt(ast: &Ast, s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
            collect_deque_arr_names_in_expr(ast, *eid, out)
        }
        Stmt::YieldInto { value, .. } => collect_deque_arr_names_in_expr(ast, *value, out),
        Stmt::Return(Some(eid)) => {
            collect_value_flow_idents(ast, *eid, out);
            collect_deque_arr_names_in_expr(ast, *eid, out);
        }
        Stmt::Return(None) => {}
        Stmt::LetDecl { name, init, .. } => {
            // `let y = xs` (or `= cond ? xs : zs` etc.) — y aliases
            // every value-flow source; all involved bindings marked.
            let mut sources = HashSet::new();
            collect_value_flow_idents(ast, *init, &mut sources);
            if !sources.is_empty() {
                out.insert(name.clone());
                out.extend(sources);
            }
            collect_deque_arr_names_in_expr(ast, *init, out);
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_deque_arr_names_in_expr(ast, *cond, out);
            collect_deque_arr_names_in_stmt(ast, then_branch, out);
            if let Some(eb) = else_branch {
                collect_deque_arr_names_in_stmt(ast, eb, out);
            }
        }
        Stmt::Labeled { body, .. } => {
            collect_deque_arr_names_in_stmt(ast, body, out);
        }
        Stmt::While { cond, body } => {
            collect_deque_arr_names_in_expr(ast, *cond, out);
            collect_deque_arr_names_in_stmt(ast, body, out);
        }
        Stmt::DoWhile { body, cond } => {
            collect_deque_arr_names_in_stmt(ast, body, out);
            collect_deque_arr_names_in_expr(ast, *cond, out);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                collect_deque_arr_names_in_stmt(ast, i, out);
            }
            if let Some(c) = cond {
                collect_deque_arr_names_in_expr(ast, *c, out);
            }
            if let Some(st) = step {
                collect_deque_arr_names_in_expr(ast, *st, out);
            }
            collect_deque_arr_names_in_stmt(ast, body, out);
        }
        Stmt::ForOf {
            src_ident, body, ..
        } => {
            // Runtime iter path unaudited for head mutation —
            // conservative mark. Narrowing this is a follow-up
            // sub-step (the runtime walks 0..length without head bump).
            out.insert(src_ident.clone());
            collect_deque_arr_names_in_stmt(ast, body, out);
        }
        Stmt::ForOfSplitIter { body, .. } => {
            // Source is always Str, not Array — no escape registration.
            collect_deque_arr_names_in_stmt(ast, body, out);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                collect_deque_arr_names_in_stmt(ast, s, out);
            }
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            collect_deque_arr_names_in_expr(ast, *scrutinee, out);
            for c in cases {
                collect_deque_arr_names_in_expr(ast, c.value, out);
                for s in &c.body {
                    collect_deque_arr_names_in_stmt(ast, s, out);
                }
            }
            if let Some(d) = default {
                for s in d {
                    collect_deque_arr_names_in_stmt(ast, s, out);
                }
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body {
                collect_deque_arr_names_in_stmt(ast, s, out);
            }
            for s in catch_body {
                collect_deque_arr_names_in_stmt(ast, s, out);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    collect_deque_arr_names_in_stmt(ast, s, out);
                }
            }
        }
        _ => {}
    }
}

fn collect_deque_arr_names_in_expr(ast: &Ast, eid: ExprId, out: &mut HashSet<String>) {
    match ast.get_expr(eid) {
        Expr::Closure { captures, .. } => {
            // Closure may shift any captured array.
            for c in captures {
                out.insert(c.clone());
            }
        }
        Expr::Call { callee, args } => {
            // `xs.shift()` / `xs.unshift(v)` — direct head mutation.
            // Any Ident arg to any call — callee may pass it on to shift.
            if let Expr::Member { obj, name } = ast.get_expr(*callee)
                && (name == "shift" || name == "unshift")
                && let Expr::Ident(n) = ast.get_expr(*obj)
            {
                out.insert(n.clone());
            }
            for a in args {
                collect_value_flow_idents(ast, *a, out);
                collect_deque_arr_names_in_expr(ast, *a, out);
            }
            collect_deque_arr_names_in_expr(ast, *callee, out);
        }
        Expr::Assign { target, value } => {
            match ast.get_expr(*target) {
                Expr::Member { .. } | Expr::Index { .. } => {
                    collect_value_flow_idents(ast, *value, out);
                }
                Expr::Ident(name) => {
                    let mut sources = HashSet::new();
                    collect_value_flow_idents(ast, *value, &mut sources);
                    if !sources.is_empty() {
                        out.insert(name.clone());
                        out.extend(sources);
                    }
                }
                _ => {}
            }
            collect_deque_arr_names_in_expr(ast, *target, out);
            collect_deque_arr_names_in_expr(ast, *value, out);
        }
        Expr::Array(els) => {
            for e in els {
                collect_value_flow_idents(ast, *e, out);
                collect_deque_arr_names_in_expr(ast, *e, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                collect_value_flow_idents(ast, *e, out);
                collect_deque_arr_names_in_expr(ast, *e, out);
            }
        }
        Expr::Spread { expr } => {
            collect_value_flow_idents(ast, *expr, out);
            collect_deque_arr_names_in_expr(ast, *expr, out);
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for e in args {
                collect_value_flow_idents(ast, *e, out);
                collect_deque_arr_names_in_expr(ast, *e, out);
            }
        }
        Expr::BinOp { left, right, .. } => {
            collect_deque_arr_names_in_expr(ast, *left, out);
            collect_deque_arr_names_in_expr(ast, *right, out);
        }
        Expr::Unary { expr, .. } | Expr::TypeOf { expr } => {
            collect_deque_arr_names_in_expr(ast, *expr, out);
        }
        Expr::InstanceOf { expr, rhs } => {
            collect_deque_arr_names_in_expr(ast, *expr, out);
            collect_deque_arr_names_in_expr(ast, *rhs, out);
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            collect_deque_arr_names_in_expr(ast, *obj, out);
        }
        Expr::OptIndex { obj, index } => {
            collect_deque_arr_names_in_expr(ast, *obj, out);
            collect_deque_arr_names_in_expr(ast, *index, out);
        }
        Expr::OptCall { callee, args } => {
            collect_deque_arr_names_in_expr(ast, *callee, out);
            for a in args {
                collect_deque_arr_names_in_expr(ast, *a, out);
            }
        }
        Expr::Index { obj, index } => {
            collect_deque_arr_names_in_expr(ast, *obj, out);
            collect_deque_arr_names_in_expr(ast, *index, out);
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_deque_arr_names_in_expr(ast, *cond, out);
            collect_deque_arr_names_in_expr(ast, *then_branch, out);
            collect_deque_arr_names_in_expr(ast, *else_branch, out);
        }
        Expr::Nullish { lhs, rhs } => {
            collect_deque_arr_names_in_expr(ast, *lhs, out);
            collect_deque_arr_names_in_expr(ast, *rhs, out);
        }
        Expr::Sequence { left, right } => {
            collect_deque_arr_names_in_expr(ast, *left, out);
            collect_deque_arr_names_in_expr(ast, *right, out);
        }
        Expr::As { expr, .. } => {
            collect_deque_arr_names_in_expr(ast, *expr, out);
        }
        Expr::PostIncr { target, .. } => {
            collect_deque_arr_names_in_expr(ast, *target, out);
        }
        _ => {}
    }
}
