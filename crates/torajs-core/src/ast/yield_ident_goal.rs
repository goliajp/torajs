//! `yield`-as-identifier goal triage — ES §12.7.2.
//!
//! Outside a generator body, `yield` is a valid BindingIdentifier /
//! IdentifierReference under the SLOPPY goal and a reserved word
//! under the STRICT goal. The parser cannot judge this — the goal
//! bit (`ast.program_is_sloppy()`, keyed on the `.cts` extension per
//! the bun mapping) is stamped after parsing — so it admits the
//! identifier and records each site in `ast.yield_ident_positions`
//! (the rotation-372 `delete <bare name>` pattern). This raw-AST
//! gate, run in the prelude right after the delete triage, raises
//! the strict-goal SyntaxError; under the sloppy goal the admitted
//! identifiers are already exactly right and nothing moves.

use super::Ast;

/// `Some(msg)` = strict-goal SyntaxError (the caller reports it as a
/// parse error and stops). Sloppy goal always answers `None`.
pub fn triage_yield_idents(ast: &Ast) -> Option<String> {
    if ast.program_is_sloppy() {
        return None;
    }
    ast.yield_ident_positions.first().map(|at| {
        format!(
            "`yield` is a reserved word in strict code (a module, or a `\"use strict\"` prologue) at {at} (ES §12.7.2)"
        )
    })
}
