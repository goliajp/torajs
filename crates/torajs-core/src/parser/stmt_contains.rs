//! Statement-level early errors that look through nested statements but
//! stop at a function boundary.
//!
//! Both rules are about the statement list itself, not about values:
//! a label may not be reused by a statement nested inside the statement
//! it labels (§14.13.1 ContainsDuplicateLabels), and a class static
//! block has no `return` (§15.7: ClassStaticBlockStatementList is
//! `StatementList[~Yield, +Await, ~Return]`). A nested function starts a
//! new label set and owns its own `return`, so the walk does not enter
//! one. Arrow and function expressions are never reached at all: their
//! bodies live in the expression arena, and this walk only follows
//! statements.

use super::*;
use crate::ast_closure_param_tag::push_child_stmts;

/// Whether `s`, or a statement nested in it on this side of a function
/// boundary, satisfies `pred`.
fn any_stmt_in(s: &Stmt, pred: &dyn Fn(&Stmt) -> bool) -> bool {
    let mut stack = vec![s];
    while let Some(s) = stack.pop() {
        if pred(s) {
            return true;
        }
        if !matches!(s, Stmt::FnDecl { .. }) {
            push_child_stmts(s, &mut stack);
        }
    }
    false
}

impl Parser<'_> {
    /// `label: body` where `body` labels a nested statement `label`
    /// again (`x: { x: 0; }`).
    pub(super) fn reject_duplicate_label(&self, label: &str, body: &Stmt) -> Result<(), String> {
        if any_stmt_in(
            body,
            &|s| matches!(s, Stmt::Labeled { label: l, .. } if l == label),
        ) {
            return Err(format!(
                "label `{label}` is already declared by an enclosing statement at {} \
                 (ES §14.13.1)",
                self.at()
            ));
        }
        Ok(())
    }

    /// A `return` anywhere in a static block's own statements.
    pub(super) fn reject_static_block_return(&self, stmts: &[Stmt]) -> Result<(), String> {
        if stmts
            .iter()
            .any(|s| any_stmt_in(s, &|s| matches!(s, Stmt::Return(_))))
        {
            return Err(format!(
                "a class static block cannot contain `return` at {} (ES §15.7)",
                self.at()
            ));
        }
        Ok(())
    }
}
