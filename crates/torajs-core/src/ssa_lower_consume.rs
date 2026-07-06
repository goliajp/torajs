//! Affine-consume helper for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 377.
//!
//! One locals-move marker remains: `consume_all_idents_in_return`
//! walks the entire expression tree under a `Stmt::Return` /
//! `Stmt::Throw` and marks every non-Copy ident it reaches as moved,
//! so the scope-exit drop walk skips locals whose heap may be aliased
//! by the escaping value. The per-arg `consume_if_ident` sibling was
//! retired by RFC 20260705 ledger #3 (chunks 564-572): call args
//! share — runtime helpers borrow (or internally inc) their args, so
//! stealing the source binding's stake either leaked it or dangled it.

use crate::ast::{Expr, ExprId};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// Walk the entire expression tree under `eid` and mark every
    /// non-Copy `Expr::Ident(name)` reference as moved. Used at
    /// `Stmt::Return` so the drop walk skips any local whose heap
    /// might be aliased by the returned value (`return helper(f)`
    /// returns the same heap as `f` — dropping `f` before the return
    /// would dangle the pointer the caller is about to receive).
    /// Conservative: marks all non-Copy idents reached, even if not
    /// actually aliased — at the return site this is safe because
    /// the locals are about to go out of scope anyway. Stops at
    /// closure / arrow bodies (their captured names live in a
    /// separate frame).
    pub(crate) fn consume_all_idents_in_return(&mut self, eid: ExprId) {
        let mut stack: Vec<ExprId> = vec![eid];
        let mut visited: std::collections::HashSet<u32> = std::collections::HashSet::new();
        while let Some(id) = stack.pop() {
            if !visited.insert(id.0) {
                continue;
            }
            match self.ast.get_expr(id).clone() {
                Expr::Ident(name) => {
                    if let Some(info) = self.locals.get_mut(&name)
                        && !info.ty.is_copy()
                    {
                        info.moved = true;
                    }
                }
                Expr::BinOp { left, right, .. } => {
                    stack.push(left);
                    stack.push(right);
                }
                Expr::Unary { expr, .. }
                | Expr::TypeOf { expr }
                | Expr::Spread { expr }
                | Expr::InstanceOf { expr, .. } => {
                    stack.push(expr);
                }
                Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
                    stack.push(obj);
                }
                Expr::Call { .. } => {
                    // RFC 20260705 owned-result invariant: a Call
                    // result owns its own ref (+1-result / fresh
                    // alloc / owned-result inc at the borrow sites),
                    // so receiver and args participate as borrows
                    // and keep their normal scope drops. Descending
                    // here double-counted: `return a.sort()` marked
                    // `a` moved while the lowering now also inc's
                    // the chaining result.
                }
                Expr::Assign { target, value } => {
                    stack.push(target);
                    stack.push(value);
                }
                Expr::Index { obj, index } => {
                    stack.push(obj);
                    stack.push(index);
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
                Expr::Nullish { lhs, rhs } => {
                    stack.push(lhs);
                    stack.push(rhs);
                }
                Expr::New { .. } => {
                    // RFC 20260705 owned-result invariant: a New
                    // result is a fresh owned allocation; ctor args
                    // participate per the ctor's own consume policy
                    // (`__new_*` consuming-params bitmap), so the
                    // return walk must not force-move them. Subsumes
                    // the prior T-26 WeakRef/WeakMap/WeakSet + P6.1
                    // Map/Set skip carve-outs.
                }
                Expr::Super { args } => {
                    for e in args {
                        stack.push(e);
                    }
                }
                Expr::PostIncr { target, .. } => {
                    stack.push(target);
                }
                _ => {}
            }
        }
    }
}
