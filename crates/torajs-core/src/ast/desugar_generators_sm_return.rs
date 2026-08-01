//! S2.30 return-completion helpers for the generator state machine —
//! the `{ value, done: true }` done-step builder and the nested-return
//! rewriter. Split out of `desugar_generators_sm` when the RFC
//! 20260802 try-region arm pushed that file past the 500-line HARD
//! limit; both methods are consumed by [`GenSm::lower`] and the
//! sibling `desugar_generators_sm_try`.

use super::desugar_generators_sm::GenSm;
use super::{Expr, ExprId, Stmt};

impl GenSm<'_> {
    /// S2.30 — build the `{ value: v, done: true }` step object a user
    /// `return v;` completes with (§27.5.1.2 return completion; `None`
    /// = bare `return;` → `undefined`). Value routing mirrors
    /// [`Self::emit_yield_return`] (Default-Any generators box via
    /// `As any`). No state store: entry already stamped the dead
    /// sentinel (RESUME_LOCAL doc), so any non-yield exit completes.
    pub(super) fn make_done_step(&mut self, v: Option<ExprId>) -> ExprId {
        let val = match v {
            Some(e) => e,
            None => self.ast.add_expr(Expr::Ident("undefined".into())),
        };
        let val_for_step = if self.yield_ty == "any" {
            self.ast.add_expr(Expr::As {
                expr: val,
                ty_ann: "any".into(),
            })
        } else {
            val
        };
        let done = self.ast.add_expr(Expr::Bool(true));
        self.ast.add_expr(Expr::ObjectLit {
            fields: vec![("value".into(), val_for_step), ("done".into(), done)],
        })
    }

    /// S2.30 — rewrite every `return v;` nested inside an
    /// inline-emitted statement (a yield-free If / While / For body,
    /// or any stmt the catch-all passes through verbatim) into the
    /// done-step return the top-level `Stmt::Return` arm produces.
    /// Walks the Stmt tree only — a nested closure body lives behind
    /// an ExprId (a different function), so it is never visited.
    pub(super) fn rewrite_nested_returns(&mut self, s: &mut Stmt) {
        match s {
            Stmt::Return(v) => {
                // D3a — under a try/finally frame the return routes
                // through F's return copy. Inline positions reached
                // here are continue-safe by construction: the finally
                // gate walker falls back on any return inside an
                // inner loop before a frame ever goes live over it.
                if !self.finally_ret.is_empty() {
                    let val = v
                        .take()
                        .unwrap_or_else(|| self.ast.add_expr(Expr::Ident("undefined".into())));
                    *s = Stmt::Block(self.build_finally_ret_stmts(val));
                    return;
                }
                let obj = self.make_done_step(v.take());
                *s = Stmt::Return(Some(obj));
            }
            Stmt::Block(ss) | Stmt::Multi(ss) => {
                for x in ss {
                    self.rewrite_nested_returns(x);
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.rewrite_nested_returns(then_branch);
                if let Some(e) = else_branch {
                    self.rewrite_nested_returns(e);
                }
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::For { body, .. }
            | Stmt::ForOf { body, .. }
            | Stmt::ForOfSplitIter { body, .. }
            | Stmt::Labeled { body, .. } => self.rewrite_nested_returns(body),
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    for x in &mut c.body {
                        self.rewrite_nested_returns(x);
                    }
                }
                if let Some(ds) = default {
                    for x in ds {
                        self.rewrite_nested_returns(x);
                    }
                }
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                for x in body.iter_mut().chain(catch_body.iter_mut()) {
                    self.rewrite_nested_returns(x);
                }
                if let Some(fs) = finally_body {
                    for x in fs {
                        self.rewrite_nested_returns(x);
                    }
                }
            }
            _ => {}
        }
    }
}
