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
        // RFC 20260801-arguments-escape-face — a BARE
        // `return __torajs_arguments` (root Ident) transfers the
        // materialized array itself to the caller; it must take the
        // moved mark like any local or the scope drop frees the heap
        // the caller just received (knife-1 probe: the returned
        // arguments array read back the NEXT allocation's bytes).
        // The unconditional skip below only guards NON-root touches
        // (index reads retain at the root / feed consuming nodes),
        // which never hand the array itself out.
        let bare_arguments_root = matches!(
            self.ast.get_expr(eid), Expr::Ident(n) if n == "__torajs_arguments"
        );
        let mut stack: Vec<ExprId> = vec![eid];
        let mut visited: std::collections::HashSet<u32> = std::collections::HashSet::new();
        while let Some(id) = stack.pop() {
            if !visited.insert(id.0) {
                continue;
            }
            match self.ast.get_expr(id).clone() {
                Expr::Ident(name) => {
                    // RFC 20260708-closure-argv-face — the
                    // materialized `__torajs_arguments` array is
                    // never transferred by a NON-root return touch
                    // (its index reads either retain at the return
                    // root or feed a consuming node); marking it
                    // moved stranded one array per call. It keeps
                    // its scope drop unless the return root IS the
                    // bare array (see above).
                    if name == "__torajs_arguments" && !bare_arguments_root {
                        continue;
                    }
                    if let Some(info) = self.locals.get_mut(&name)
                        && !info.ty.is_copy()
                    {
                        info.moved = true;
                    }
                }
                Expr::BinOp { left, right, .. } => {
                    // Chunk 718 — a BinOp answers a FRESH value
                    // (arithmetic, concat, fresh any box), never an
                    // alias of an Index receiver's heap. Descending
                    // into an Index operand marked its receiver moved
                    // and stranded the whole container per call
                    // (`return a[0] + a[1]` leaked the array — probe
                    // p717b 42.8MB vs 6.4MB flat, chunk-674 residual
                    // face). Skip Index operands wholesale (the read
                    // borrows; the receiver keeps its scope drop);
                    // every other operand shape keeps the
                    // conservative walk. Root-position `return a[i]`
                    // is untouched (elem-borrow returns still pin
                    // their receiver).
                    for side in [left, right] {
                        if !matches!(self.ast.get_expr(side), Expr::Index { .. }) {
                            stack.push(side);
                        }
                    }
                }
                Expr::Unary { expr, .. } | Expr::TypeOf { expr } | Expr::Spread { expr } => {
                    stack.push(expr);
                }
                Expr::InstanceOf { expr, rhs } => {
                    stack.push(expr);
                    stack.push(rhs);
                }
                Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
                    // Chunk 752 — an any-member read answers an OWNED
                    // box (chunk 717 contract; OptChain mints a fresh
                    // owned box likewise), never an alias of the
                    // receiver's heap — same owned-result invariant
                    // as the Call / New arms. Descending marked the
                    // receiver moved and stranded its cell per call
                    // (`return v.length` with `v: any` leaked the
                    // concat cell — probe vE 15.97MB vs 6.37MB flat,
                    // while the non-return read vM stayed flat).
                    // Non-Any receivers keep the conservative walk
                    // (a struct field read may borrow).
                    let any_owned_answer = if let Expr::Ident(name) = self.ast.get_expr(obj) {
                        self.locals
                            .get(name)
                            .is_some_and(|info| info.ty == crate::ssa::Type::Any)
                    } else {
                        false
                    };
                    if !any_owned_answer {
                        stack.push(obj);
                    }
                }
                Expr::OptIndex { obj, index } => {
                    stack.push(obj);
                    stack.push(index);
                }
                Expr::Call { .. } | Expr::OptCall { .. } => {
                    // RFC 20260705 owned-result invariant: a Call
                    // result owns its own ref (+1-result / fresh
                    // alloc / owned-result inc at the borrow sites),
                    // so receiver and args participate as borrows
                    // and keep their normal scope drops. Descending
                    // here double-counted: `return a.sort()` marked
                    // `a` moved while the lowering now also inc's
                    // the chaining result. OptCall answers a fresh
                    // owned Any box — same story.
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
