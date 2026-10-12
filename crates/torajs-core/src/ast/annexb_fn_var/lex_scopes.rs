//! The lexical scopes between a var scope and the declaration the lift
//! is looking at.
//!
//! §B.3.3.1 step 1.a.ii applies Annex B only when replacing the
//! declaration with `var F` would raise no early error, and a `let` /
//! `const` of the same name in any enclosing block is exactly that
//! error — so a hit here skips the whole thing, var binding included.
//! A function declared in an enclosing BLOCK is lexical there too
//! (§14.2.6), so it counts the same way: in
//! `{ function f() {} { function f() {} } }` the inner `f` keeps its
//! block binding and writes no var (test262
//! `annexB/language/function-code/block-decl-nested-blocks-with-fun-decl.js`).

use super::super::Stmt;
use super::super::annexb_names::lex_names_into;
use crate::lexer::Span;

/// Names lexically declared by the scopes around the walk, innermost
/// last. Pushed and truncated by the walk, so it is a stack, not a set.
#[derive(Default)]
pub(in crate::ast) struct LexScopes {
    names: Vec<String>,
    /// Parallel to `names`: for a block's function declaration, the
    /// mark of the list that declares it and the declaration's span.
    /// A declaration does not collide with its own list's functions —
    /// a repeated `function f` in one block is §B.3.3.4's legal case.
    fns: Vec<Option<(usize, Span)>>,
}

impl LexScopes {
    /// Does a scope around the declaration of `name` at `span` declare
    /// the name lexically?
    pub(in crate::ast) fn owns(&self, name: &str, span: Span) -> bool {
        let own = self
            .names
            .iter()
            .zip(&self.fns)
            .rev()
            .find_map(|(n, t)| match t {
                Some((list, s)) if n == name && *s == span => Some(*list),
                _ => None,
            });
        self.names.iter().zip(&self.fns).any(|(n, t)| {
            n == name && !matches!((t, own), (Some((list, _)), Some(o)) if *list == o)
        })
    }

    /// Enter a scope: remember the stack height, then record what the
    /// statement list declares lexically. The mark goes back to
    /// [`Self::leave`] when the walk leaves.
    pub(in crate::ast) fn enter_scope(&mut self, body: &[Stmt]) -> usize {
        let mark = self.names.len();
        lex_names_into(body, &mut self.names);
        self.fns.resize(self.names.len(), None);
        mark
    }

    /// Record the function declarations of a block's list (`mark` is
    /// the list's own [`Self::enter_scope`] mark). Not for a function
    /// body's list, whose declarations are var-like.
    pub(in crate::ast) fn enter_block_fns(&mut self, body: &[Stmt], mark: usize) {
        for s in body {
            match s {
                Stmt::FnDecl { name, span, .. } => {
                    self.names.push(name.clone());
                    self.fns.push(Some((mark, *span)));
                }
                Stmt::Multi(inner) => self.enter_block_fns(inner, mark),
                _ => {}
            }
        }
    }

    /// Enter a scope that binds exactly one name — a `for` head's
    /// `let` / `const`, or a for-in / for-of element binding.
    pub(in crate::ast) fn enter_binding(&mut self, name: &str) -> usize {
        let mark = self.names.len();
        self.names.push(name.to_string());
        self.fns.push(None);
        mark
    }

    /// A scope that binds nothing — the mark alone, so both arms of a
    /// loop head can leave the same way.
    pub(in crate::ast) fn enter_binding_none(&self) -> usize {
        self.names.len()
    }

    pub(in crate::ast) fn leave(&mut self, mark: usize) {
        self.names.truncate(mark);
        self.fns.truncate(mark);
    }

    /// Start a fresh var scope. Nothing outside a function body's own
    /// lexical scopes can raise the early error §B.3.3.1 asks about.
    pub(in crate::ast) fn clear(&mut self) {
        self.names.clear();
        self.fns.clear();
    }
}
