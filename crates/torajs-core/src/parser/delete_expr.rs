//! `delete <expr>` operand triage — ES §13.5.1.
//!
//! A property reference (obj.k / obj[k]) is the real delete; a bare
//! name is the strict-mode SyntaxError §13.5.1.1 (modules are strict);
//! an optional chain stays a loud boundary (spec deletes through it —
//! silently answering true would be wrong); every other operand is not
//! a Reference Record, so §13.5.1.2 step 2 evaluates it for its
//! effects and the result is `true` (`delete 42`, `delete foo()`,
//! `delete this` — all legal in strict code, and bun runs them).
//!
//! Split out of `expr_prec.rs` (2026-08-10): that file is a registered
//! size-debt entry, so growing it in place was not available.

use super::Parser;
use crate::ast::Expr;

impl Parser<'_> {
    /// Enters with `delete` already consumed. Parses the operand and
    /// routes it to the delete node, an early error, or the
    /// non-reference true-lane.
    pub(super) fn parse_delete_operand(&mut self) -> Result<crate::ast::ExprId, String> {
        // `void <literal>` folds to the plain `undefined` Ident in the
        // arena (expr_prec), so `delete void 0` and the user-written
        // `delete undefined` arrive here IDENTICAL. The §13.5.1.1
        // early error is a SYNTAX judgement, so the source form
        // decides: an operand that opens with `void` was never an
        // IdentifierReference, whatever it folded to.
        let opens_with_void = matches!(self.peek(), crate::lexer::Token::Void);
        let inner = self.parse_unary()?;
        // §13.5.1.1 early error — the operand must not be a private
        // reference (`delete o.#m`), however the receiver is shaped.
        // Parens are transparent in the arena, so the covered forms
        // land here too. A private name parses to its
        // `__priv_<cls>__<n>` mangling (member_name_after_dot), so
        // that prefix is the marker.
        if let Expr::Member { name, .. } = self.ast.get_expr(inner)
            && (name.starts_with("__priv_") || name.starts_with("__privu_"))
        {
            let bare = name.rsplit("__").next().unwrap_or(name);
            return Err(format!(
                "deleting the private name `#{bare}` is forbidden at {}",
                self.at()
            ));
        }
        match self.ast.get_expr(inner) {
            Expr::Member { .. } | Expr::Index { .. } => {
                Ok(self.ast.add_expr(Expr::Delete { expr: inner }))
            }
            // The folded `void <literal>` — not a Reference, answers
            // true (§13.5.1.2 step 2; the literal had no effects).
            Expr::Ident(n) if n == "undefined" && opens_with_void => {
                Ok(self.ast.add_expr(Expr::Bool(true)))
            }
            // §13.5.1.1 — `delete x` on an unqualified name is the
            // strict-mode early error. The sloppy-only semantics
            // (false for a declared binding) has no surface here: tr
            // compiles modules, and bun rejects these programs the
            // same way.
            Expr::Ident(_) => Err(format!(
                "`delete` on a bare name is a SyntaxError in strict code (modules are strict) at {}",
                self.at()
            )),
            // Deleting through `?.` is a real delete guarded by the
            // nullish check (§13.5.1.2 step 3) — routing it to the
            // true-lane would silently skip the deletion.
            Expr::OptChain { .. } | Expr::OptIndex { .. } | Expr::OptCall { .. } => Err(format!(
                "`delete` through an optional chain is not supported yet at {}",
                self.at()
            )),
            // Pure literal operand: no effects to keep (the
            // `void <literal>` fold in expr_prec is the precedent).
            Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Null => {
                Ok(self.ast.add_expr(Expr::Bool(true)))
            }
            // Non-reference operand — evaluate for effects, then the
            // result is `true` (same Sequence shape as the `void`
            // desugar).
            _ => {
                let t = self.ast.add_expr(Expr::Bool(true));
                Ok(self.ast.add_expr(Expr::Sequence {
                    left: inner,
                    right: t,
                }))
            }
        }
    }
}
