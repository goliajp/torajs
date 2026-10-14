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
//! goal the admitted identifiers are already exactly right; what the
//! script goal refuses instead is a top-level AwaitExpression the
//! parser read for the module goal.

use super::Ast;

/// `Some(msg)` = goal SyntaxError (the caller reports it as a parse
/// error and stops). The module goal refuses the admitted names; the
/// script goal refuses the top-level `await` operators the parser read
/// for the module goal (`ast.module_await_positions`) whose name
/// reading cannot parse either: `await x;`, `await using x = y;`,
/// `for await (...)` at a Script's top level (§13.1.1, §15.8.1).
pub fn triage_await_idents(ast: &Ast) -> Option<String> {
    if ast.sloppy_script_goal {
        return ast.module_await_positions.first().map(|at| {
            format!(
                "`await` is only valid in async functions and at the top level of a module at \
                 {at} (ES §15.8.1)"
            )
        });
    }
    ast.await_ident_positions
        .first()
        .map(|at| format!("`await` is a reserved word in module code at {at} (ES §13.1.1)"))
}
