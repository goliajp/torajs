//! The lexical scopes between a var scope and the declaration the lift
//! is looking at.
//!
//! §B.3.3.1 step 1.a.ii applies Annex B only when replacing the
//! declaration with `var F` would raise no early error, and a `let` /
//! `const` of the same name in any enclosing block is exactly that
//! error — so a hit here skips the whole thing, var binding included.

use super::super::Stmt;
use super::super::annexb_names::lex_names_into;

/// Names lexically declared by the scopes around the walk, innermost
/// last. Pushed and truncated by the walk, so it is a stack, not a set.
#[derive(Default)]
pub(in crate::ast) struct LexScopes {
    names: Vec<String>,
}

impl LexScopes {
    /// Does an enclosing scope declare `name` lexically?
    pub(in crate::ast) fn contains(&self, name: &str) -> bool {
        self.names.iter().any(|n| n == name)
    }

    /// Enter a scope: remember the stack height, then record what the
    /// statement list declares lexically. The mark goes back to
    /// [`Self::leave`] when the walk leaves.
    pub(in crate::ast) fn enter_scope(&mut self, body: &[Stmt]) -> usize {
        let mark = self.names.len();
        lex_names_into(body, &mut self.names);
        mark
    }

    /// Enter a scope that binds exactly one name — a `for` head's
    /// `let` / `const`, or a for-in / for-of element binding.
    pub(in crate::ast) fn enter_binding(&mut self, name: &str) -> usize {
        let mark = self.names.len();
        self.names.push(name.to_string());
        mark
    }

    /// A scope that binds nothing — the mark alone, so both arms of a
    /// loop head can leave the same way.
    pub(in crate::ast) fn enter_binding_none(&self) -> usize {
        self.names.len()
    }

    pub(in crate::ast) fn leave(&mut self, mark: usize) {
        self.names.truncate(mark);
    }

    /// Start a fresh var scope. Nothing outside a function body's own
    /// lexical scopes can raise the early error §B.3.3.1 asks about.
    pub(in crate::ast) fn clear(&mut self) {
        self.names.clear();
    }
}
