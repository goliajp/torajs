//! Affine-consume helpers for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 377.
//!
//! Two locals-move markers used by SSA lowering to keep the drop walk
//! in sync with check.rs's affine consume pass: `consume_if_ident`
//! marks a single non-Copy `Ident(name)` binding as moved (no-op for
//! Copy / non-Ident); `consume_all_idents_in_return` walks the entire
//! expression tree under a `Stmt::Return` and marks every non-Copy
//! ident it reaches as moved, so the return-site drop walk skips
//! locals whose heap may be aliased by the returned value. Both
//! bodies are byte-for-byte preserved from the source; the sibling
//! reaches LowerCtx fields via `impl<'a> super::LowerCtx<'a>`, so
//! call sites need zero edits.

use crate::ast::{Expr, ExprId};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// If `eid` resolves to a non-Copy `Ident(name)` binding, mark that
    /// binding as moved. No-op for Copy types (number/bool/etc) and for
    /// non-Ident expressions (literals, BinOp results, Call results).
    /// Mirrors check.rs's affine consume pass.
    pub(crate) fn consume_if_ident(&mut self, eid: ExprId) {
        if let Expr::Ident(name) = self.ast.get_expr(eid) {
            let name = name.clone();
            if let Some(info) = self.locals.get_mut(&name)
                && !info.ty.is_copy()
            {
                info.moved = true;
            }
        }
    }

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
                Expr::Call { callee, args } => {
                    stack.push(callee);
                    for a in args {
                        stack.push(a);
                    }
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
                Expr::New { class_name, args } => {
                    /* T-26 — `new WeakRef(target)` / `new WeakMap()`
                     * / `new WeakSet()` borrow their args (or take
                     * none); skip the recurse so the consume walk
                     * doesn't mark bound idents as moved.
                     * P6.1 — `new Map()` is zero-arg; the iterable-
                     * initializer overload (P6.5) will need its own
                     * recurse policy. */
                    if class_name == "WeakRef"
                        || class_name == "WeakMap"
                        || class_name == "WeakSet"
                        || class_name == "Map"
                        || class_name == "Set"
                    {
                        continue;
                    }
                    for e in args {
                        stack.push(e);
                    }
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
