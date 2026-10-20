//! Statement lowering dispatch for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 401 — Path A.3-batch22.
//!
//! Single method `lower_stmt(s)` — the outer `match Stmt::*` dispatch
//! that routes each statement variant to its per-arm sibling module
//! (`ssa_lower_stmt_block` / `ssa_lower_stmt_if` /
//! `ssa_lower_stmt_switch` / `ssa_lower_stmt_for_of` / etc.). Covers:
//!
//! - `Multi` (compiler-generated sequence, shares parent scope,
//!   dispatches each child through the same lower_stmt recursion +
//!   `try_lower_while_fast` counter fast-path);
//! - control flow: `Block` / `If` / `While` / `DoWhile` / `For` /
//!   `ForOf` / `ForOfSplitIter` / `Switch` / `Break` / `Continue` /
//!   `Return`;
//! - exception flow: `Try` / `Throw`;
//! - declaration: `LetDecl` (mutable/type-ann/init) — `is_var: true`
//!   falls through to the catch-all (var-hoisting is compile-time
//!   TypeError in tora's flavor);
//! - `Expr` — expression as statement; shared per-arm inspect
//!   dispatch for multi-arg `console.log` (else generic
//!   `self.lower_expr(*eid)`);
//! - no-op meta forms: `TypeDecl` (pass 0 already registered),
//!   `ImportDecl` / `ExportDecl` (K.1 single-file mode);
//! - catch-all: friendly panic with a labeled classification for
//!   `FnDecl` / `ClassDecl` inside a nested scope (planned hoisting)
//!   and any other unimplemented statement shape.

use crate::ast::{Expr, Stmt};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_while_push_fast::lower_while_inner;

impl<'a> LowerCtx<'a> {
    pub(crate) fn lower_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Multi(stmts) => {
                // Compiler-generated sequence — share surrounding scope.
                // No scope push, no drop emission of its own. Each child
                // lowers as if it appeared at the parent site. Used by
                // parse-time desugars (destructuring, possibly others)
                // that need to emit multiple lets without burying them
                // in a child block.
                let mut prev: Option<&Stmt> = None;
                for s in stmts {
                    if !self.try_lower_while_fast(prev, s) {
                        self.lower_stmt(s);
                    }
                    if !self.cur_open() {
                        break;
                    }
                    prev = Some(s);
                }
            }
            Stmt::Block(stmts) => {
                crate::ssa_lower_stmt_block::lower(self, stmts);
            }
            Stmt::LetDecl {
                mutable,
                name,
                type_ann,
                init,
                is_var: false,
            } => {
                // An array this body wrote out itself, which is what
                // `PreReserve::owns_alone` needs to rule out the
                // caller having passed the same one in twice.
                self.proofs.kill(name);
                if self.makes_fresh_array(*init) {
                    self.prereserve.note_array_literal_let(name);
                }
                let outer = self.locals.get(name).map(|l| l.slot);
                crate::ssa_lower_stmt_let_decl::lower(
                    self,
                    name,
                    type_ann.as_ref(),
                    *init,
                    *mutable,
                );
                self.note_let(name, outer, type_ann.as_ref(), *init);
            }
            Stmt::While { cond, body } => {
                self.proofs.forget_lets();
                lower_while_inner(self, *cond, body, None);
                self.proofs.forget_lets();
            }
            Stmt::ForOfSplitIter {
                var_name,
                parent,
                sep,
                body,
            } => {
                self.proofs.forget_lets();
                crate::ssa_lower_stmt_for_of_split_iter::lower(self, var_name, *parent, *sep, body);
                self.proofs.forget_lets();
            }
            Stmt::ForOf {
                var_name,
                i_ident,
                elem_expr,
                body,
                forin_obj,
                is_await,
                ..
            } => {
                self.proofs.forget_lets();
                crate::ssa_lower_stmt_for_of::lower(
                    self, var_name, i_ident, *elem_expr, body, *forin_obj, *is_await,
                );
                self.proofs.forget_lets();
            }
            Stmt::DoWhile { body, cond } => {
                self.proofs.forget_lets();
                crate::ssa_lower_stmt_do_while::lower(self, body, *cond);
                self.proofs.forget_lets();
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
                default_at,
            } => {
                let saved = self.proofs.region_enter();
                crate::ssa_lower_stmt_switch::lower(self, *scrutinee, cases, default, *default_at);
                self.proofs.region_leave(&saved);
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                self.proofs.forget_lets();
                crate::ssa_lower_stmt_for::lower(self, init.as_deref(), *cond, *step, body);
                self.proofs.forget_lets();
            }
            Stmt::Break(label) => {
                crate::ssa_lower_stmt_break_continue::lower_break(self, label.as_deref());
            }
            Stmt::Continue(label) => {
                crate::ssa_lower_stmt_break_continue::lower_continue(self, label.as_deref());
            }
            Stmt::Labeled { label, body } => {
                let saved = self.proofs.region_enter();
                crate::ssa_lower_stmt_break_continue::lower_labeled(self, label, body);
                self.proofs.region_leave(&saved);
            }
            Stmt::Throw(eid) => {
                crate::ssa_lower_stmt_throw::lower(self, *eid);
            }
            Stmt::Try {
                body,
                had_catch,
                catch_param,
                catch_type,
                catch_body,
                finally_body,
            } => {
                crate::ssa_lower_stmt_try::lower(
                    self,
                    body,
                    *had_catch,
                    catch_param.as_ref(),
                    catch_type.as_ref(),
                    catch_body,
                    finally_body.as_ref(),
                );
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                crate::ssa_lower_stmt_if::lower(self, *cond, then_branch, else_branch.as_deref());
            }
            Stmt::Return(maybe) => {
                let maybe = crate::ssa_lower_fn::suspend_ret::returned_expr(self, *maybe);
                crate::ssa_lower_stmt_return::lower(self, maybe);
            }
            Stmt::Expr(eid) => {
                // Multi-arg `console.log` per-arg inspect dispatch —
                // shared with lower_top_stmt; without this the legacy
                // `coerce_to_str` joiner below would panic on typed
                // `Arr<T>` args inside try-body / nested block scopes.
                if crate::ssa_lower_console_log_multiarg::try_lower(self, s) {
                    return;
                }
                // Result discarded. Expression may still produce SSA insts as
                // side effects (its own value), e.g. nested Calls.
                //
                // A lowering that has to answer a TYPED value can use
                // this: here there is no slot, so a contract about
                // what fits in one does not apply. Only the root
                // call's callee is recorded — anything deeper feeds
                // an operand that IS read.
                let saved_discard = self.discarded_stmt_call;
                let saved_discard_expr = self.discarded_stmt_expr;
                self.discarded_stmt_call = match self.ast.get_expr(*eid) {
                    Expr::Call { callee, .. } => Some(*callee),
                    _ => None,
                };
                self.discarded_stmt_expr = Some(*eid);
                let op = self.lower_expr(*eid);
                self.discarded_stmt_call = saved_discard;
                self.discarded_stmt_expr = saved_discard_expr;
                // RFC 20260705 owned-result invariant — the shape
                // predicate + release live in `release_owned_temp`
                // (ssa_lower_rc_emit); Ternary/Nullish over Calls
                // stay on the L3b ledger.
                self.release_owned_temp(*eid, &op);
            }
            Stmt::TypeDecl { .. } => {
                // Pass 0 of `lower()` already registered the alias and
                // interned the layout. Re-encountering during the body
                // walk is a no-op.
            }
            Stmt::ImportDecl { .. } => {
                // K.1 single-file mode: no semantic effect. K.2 will
                // wire this into a cross-file symbol table.
            }
            Stmt::ExportDecl { .. } => {
                // K.1: `unwrap_exports` desugar should have flattened
                // the declaration-form. Bare named-export (`export {
                // ... }`) reaches here and is a no-op in single-file
                // mode.
            }
            other => panic!("{}", unimplemented_stmt_label(other)),
        }
    }
}

/// Friendly classification for the most common shapes that hit the
/// catch-all so users get a readable message instead of the raw AST
/// debug print.
fn unimplemented_stmt_label(other: &Stmt) -> String {
    match other {
        Stmt::FnDecl { name, .. } => format!(
            "nested function declaration `{name}` inside a block / switch (planned: function-statement hoisting, see roadmap)"
        ),
        Stmt::ClassDecl { name, .. } => format!(
            "nested class declaration `{name}` inside a block (planned: same hoisting story as nested functions)"
        ),
        _ => format!("statement shape not yet implemented: {other:?}"),
    }
}

/// Array methods that answer an array no other name holds yet.
const FRESH_ARRAY_METHODS: [&str; 10] = [
    "map",
    "filter",
    "slice",
    "concat",
    "flat",
    "flatMap",
    "toSorted",
    "toReversed",
    "toSpliced",
    "with",
];

impl LowerCtx<'_> {
    /// `init` builds a new array: a literal, an array method that
    /// answers a copy, or a string split. No caller and no other
    /// binding can be holding the cell it makes.
    fn makes_fresh_array(&self, init: crate::ast::ExprId) -> bool {
        use crate::ast::Expr;
        use crate::check::Type as T;
        match self.ast.get_expr(init) {
            Expr::Array(_) => true,
            Expr::Call { callee, .. } => {
                let Expr::Member { obj, name } = self.ast.get_expr(*callee) else {
                    return false;
                };
                match self.expr_types.get(obj) {
                    Some(T::Array(_)) => FRESH_ARRAY_METHODS.contains(&name.as_str()),
                    Some(T::String) => name == "split",
                    _ => false,
                }
            }
            _ => false,
        }
    }
}
