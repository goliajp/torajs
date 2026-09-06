//! The provenance question the three undeclared-name sites ask before
//! they treat an unresolved `__` name as the program's own.
//!
//! Split out of the parent when the dead-zone table pushed it past the
//! 500-line cap (rotation 603). Pure move — an inherent `impl` in a
//! child module is the same method, and the parent keeps every caller.

use super::Checker;

impl Checker {
    /// Is this name one TR minted, rather than one the program spelled?
    ///
    /// The three undeclared-name sites (read, write, unresolved
    /// capture) give an unresolvable identifier the §6.2.5.5 /
    /// §6.2.5.6 posture — types `Any`, one deduped warning, a
    /// catchable `ReferenceError` when evaluated — and carve out the
    /// names tr synthesizes, because an unresolved one of those is a
    /// compiler bug that a runtime throw would bury. `__` is the
    /// prefix tr mints with, but it is not a reserved namespace and it
    /// cannot answer this question on its own: sputnik's half of
    /// test262 spells its user identifiers `__ref` / `__key` /
    /// `__func` throughout. What answers it is provenance —
    /// `ast::record_source_dunder_idents` records the `__` names the
    /// LEXER saw, which is the only boundary the parser's own minting
    /// (`__torajs_in_op`, `__priv_*`) does not sit inside.
    pub(crate) fn is_synthesized_dunder(&self, name: &str) -> bool {
        name.starts_with("__") && !self.source_dunder_idents.contains(name)
    }
}
