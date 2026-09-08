//! `await`-as-identifier goal triage — ES §13.1.1 / §15.8.1.
//!
//! Outside an async body, `await` is a valid BindingIdentifier under
//! the SCRIPT goal and reserved under the MODULE goal. Strictness has
//! nothing to do with it: `"use strict"; var await = 1;` is an
//! ordinary declaration in a Script, which is what separates this
//! from the `yield` lane next door.
//!
//! The parser cannot judge this — `ast.sloppy_script_goal` is stamped
//! after parsing — so it admits the name in binding positions and
//! records each site in `ast.await_ident_positions` (the rotation-374
//! `yield` pattern). This raw-AST gate, run in the prelude beside the
//! yield triage, raises the module-goal SyntaxError; under the script
//! goal the admitted identifiers are already exactly right and
//! nothing moves.

use super::Ast;

/// `Some(msg)` = module-goal SyntaxError (the caller reports it as a
/// parse error and stops). Script goal always answers `None`.
pub fn triage_await_idents(ast: &Ast) -> Option<String> {
    if ast.sloppy_script_goal {
        return None;
    }
    ast.await_ident_positions
        .first()
        .map(|at| format!("`await` is a reserved word in module code at {at} (ES §13.1.1)"))
}
