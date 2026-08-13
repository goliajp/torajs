//! 11-A2-a — fn-local escape analysis for `Obj`-typed bindings
//! initialised by `ObjectLit`.
//!
//! Walks the fn body's AST and collects binding names whose live
//! object value **may** escape the local scope (return value /
//! callee retention / store-to-heap / alias / closure capture /
//! iter source / control-flow lift). Conservative by design:
//! false-positive = miss a stack-alloc opportunity, false-negative
//! = silent-wrong stack-use-after-free; only the former is
//! acceptable.
//!
//! Returns the set of "obj-escape" binding names. The complementary
//! "safe to swap heap obj_alloc for stack AllocaBytes" decision is
//! made at the `LetDecl` alloc emit site: a binding whose init is
//! `ObjectLit { … }` and whose name `∉ escape_obj_lets` is stack-
//! alloced; everything else stays on the heap.
//!
//! First-version escape patterns covered:
//! - `return X` — escapes through caller
//! - `obj.f = X` / `arr[i] = X` — escape through heap storage
//! - array literal `[a, X, b]` / object literal `{f: X}` — heap
//!   storage
//! - `...X` spread — copy into a heap container
//! - `let Y = X` / `Y = X` — alias propagates (mark both X and Y;
//!   conservative — no per-binding precision); value-transparent
//!   wrappers are looked through (`cond ? X : Z` / `X ?? Z` /
//!   `(e, X)` / `X as T` / nested assign) at every escape site —
//!   see `collect_value_flow_idents` in the deque-escape sibling
//! - `foo(X)` — fn-call arg, callee may retain
//! - `X.method(...)` — receiver passed to method, callee may retain
//! - `new C(X)` / `super(X)` — constructor arg
//! - `for (... of X)` source — runtime iter path unaudited
//! - closure capture of `X` — lifted body may retain
//! - `throw X` / `yield X` — escape via control flow (exception
//!   propagation / generator suspension)
//!
//! Patterns explicitly *not* escape-bound (the safe set):
//! - `X.field` / `X[i]` — read-only access, no escape
//! - `X.field = v` — write into X's slot, doesn't expose X itself
//!   (rhs `v` may itself be escape-bound, handled separately when
//!   `v` is an `Ident`)
//! - `X == null` / `typeof X` — read-only test
//! - arithmetic / comparison on `X.field` — reads only
//!
//! Lives in its own file (not collocated with the other AST
//! visitors in `ssa_lower.rs`) so `ssa_lower.rs` stays on the
//! "known-debt only-shrinks" trajectory; future sub-steps of the
//! `ssa_lower` carve-out plan move the other visitors here too.

use std::collections::HashSet;

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::ssa_lower_deque_escape::collect_value_flow_idents;

pub(crate) fn collect_escape_obj_let_names_in_stmt(ast: &Ast, s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::Expr(eid) => collect_escape_obj_let_names_in_expr(ast, *eid, out),
        Stmt::Throw(eid) | Stmt::Yield(eid) => {
            collect_value_flow_idents(ast, *eid, out);
            collect_escape_obj_let_names_in_expr(ast, *eid, out);
        }
        Stmt::YieldInto { value, .. } => {
            collect_value_flow_idents(ast, *value, out);
            collect_escape_obj_let_names_in_expr(ast, *value, out);
        }
        Stmt::Return(Some(eid)) => {
            collect_value_flow_idents(ast, *eid, out);
            collect_escape_obj_let_names_in_expr(ast, *eid, out);
        }
        Stmt::Return(None) => {}
        Stmt::LetDecl { name, init, .. } => {
            // `let y = X` (or `= cond ? X : Z` etc.) — y aliases
            // every value-flow source; all involved bindings marked.
            let mut sources = HashSet::new();
            collect_value_flow_idents(ast, *init, &mut sources);
            if !sources.is_empty() {
                out.insert(name.clone());
                out.extend(sources);
            }
            collect_escape_obj_let_names_in_expr(ast, *init, out);
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_escape_obj_let_names_in_expr(ast, *cond, out);
            collect_escape_obj_let_names_in_stmt(ast, then_branch, out);
            if let Some(eb) = else_branch {
                collect_escape_obj_let_names_in_stmt(ast, eb, out);
            }
        }
        Stmt::Labeled { body, .. } => {
            collect_escape_obj_let_names_in_stmt(ast, body, out);
        }
        Stmt::While { cond, body } => {
            collect_escape_obj_let_names_in_expr(ast, *cond, out);
            collect_escape_obj_let_names_in_stmt(ast, body, out);
        }
        Stmt::DoWhile { body, cond } => {
            collect_escape_obj_let_names_in_stmt(ast, body, out);
            collect_escape_obj_let_names_in_expr(ast, *cond, out);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                collect_escape_obj_let_names_in_stmt(ast, i, out);
            }
            if let Some(c) = cond {
                collect_escape_obj_let_names_in_expr(ast, *c, out);
            }
            if let Some(st) = step {
                collect_escape_obj_let_names_in_expr(ast, *st, out);
            }
            collect_escape_obj_let_names_in_stmt(ast, body, out);
        }
        Stmt::ForOf {
            src_ident, body, ..
        } => {
            // Runtime iter path unaudited for obj retention —
            // conservative mark.
            out.insert(src_ident.clone());
            collect_escape_obj_let_names_in_stmt(ast, body, out);
        }
        Stmt::ForOfSplitIter { body, .. } => {
            // Source is always Str, not Obj — no escape registration.
            collect_escape_obj_let_names_in_stmt(ast, body, out);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                collect_escape_obj_let_names_in_stmt(ast, s, out);
            }
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            collect_escape_obj_let_names_in_expr(ast, *scrutinee, out);
            for c in cases {
                collect_escape_obj_let_names_in_expr(ast, c.value, out);
                for s in &c.body {
                    collect_escape_obj_let_names_in_stmt(ast, s, out);
                }
            }
            if let Some(d) = default {
                for s in d {
                    collect_escape_obj_let_names_in_stmt(ast, s, out);
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
                collect_escape_obj_let_names_in_stmt(ast, s, out);
            }
            for s in catch_body {
                collect_escape_obj_let_names_in_stmt(ast, s, out);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    collect_escape_obj_let_names_in_stmt(ast, s, out);
                }
            }
        }
        _ => {}
    }
}

fn collect_escape_obj_let_names_in_expr(ast: &Ast, eid: ExprId, out: &mut HashSet<String>) {
    match ast.get_expr(eid) {
        Expr::Closure { captures, .. } => {
            for c in captures {
                out.insert(c.clone());
            }
        }
        Expr::Call { callee, args } => {
            // `X.method(...)` — receiver passed to method, callee
            // may retain. Mark any Ident receiver of a method call.
            if let Expr::Member { obj, .. } = ast.get_expr(*callee) {
                collect_value_flow_idents(ast, *obj, out);
            }
            for a in args {
                collect_value_flow_idents(ast, *a, out);
                collect_escape_obj_let_names_in_expr(ast, *a, out);
            }
            collect_escape_obj_let_names_in_expr(ast, *callee, out);
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
            collect_escape_obj_let_names_in_expr(ast, *target, out);
            collect_escape_obj_let_names_in_expr(ast, *value, out);
        }
        Expr::Array(els) => {
            for e in els {
                collect_value_flow_idents(ast, *e, out);
                collect_escape_obj_let_names_in_expr(ast, *e, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                collect_value_flow_idents(ast, *e, out);
                collect_escape_obj_let_names_in_expr(ast, *e, out);
            }
        }
        Expr::Spread { expr } => {
            collect_value_flow_idents(ast, *expr, out);
            collect_escape_obj_let_names_in_expr(ast, *expr, out);
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for e in args {
                collect_value_flow_idents(ast, *e, out);
                collect_escape_obj_let_names_in_expr(ast, *e, out);
            }
        }
        Expr::BinOp { left, right, .. } => {
            collect_escape_obj_let_names_in_expr(ast, *left, out);
            collect_escape_obj_let_names_in_expr(ast, *right, out);
        }
        Expr::Unary { expr, .. } | Expr::TypeOf { expr } => {
            collect_escape_obj_let_names_in_expr(ast, *expr, out);
        }
        Expr::InstanceOf { expr, rhs } => {
            collect_escape_obj_let_names_in_expr(ast, *expr, out);
            collect_escape_obj_let_names_in_expr(ast, *rhs, out);
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            collect_escape_obj_let_names_in_expr(ast, *obj, out);
        }
        Expr::OptIndex { obj, index } => {
            collect_escape_obj_let_names_in_expr(ast, *obj, out);
            collect_escape_obj_let_names_in_expr(ast, *index, out);
        }
        Expr::OptCall { callee, args } => {
            collect_escape_obj_let_names_in_expr(ast, *callee, out);
            for a in args {
                collect_escape_obj_let_names_in_expr(ast, *a, out);
            }
        }
        Expr::Index { obj, index } => {
            collect_escape_obj_let_names_in_expr(ast, *obj, out);
            collect_escape_obj_let_names_in_expr(ast, *index, out);
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_escape_obj_let_names_in_expr(ast, *cond, out);
            collect_escape_obj_let_names_in_expr(ast, *then_branch, out);
            collect_escape_obj_let_names_in_expr(ast, *else_branch, out);
        }
        Expr::Nullish { lhs, rhs } => {
            collect_escape_obj_let_names_in_expr(ast, *lhs, out);
            collect_escape_obj_let_names_in_expr(ast, *rhs, out);
        }
        Expr::Sequence { left, right } => {
            collect_escape_obj_let_names_in_expr(ast, *left, out);
            collect_escape_obj_let_names_in_expr(ast, *right, out);
        }
        Expr::As { expr, .. } => {
            collect_escape_obj_let_names_in_expr(ast, *expr, out);
        }
        Expr::PostIncr { target, .. } => {
            collect_escape_obj_let_names_in_expr(ast, *target, out);
        }
        _ => {}
    }
}
