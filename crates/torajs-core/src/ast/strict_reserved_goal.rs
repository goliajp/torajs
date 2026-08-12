//! Names strict mode code refuses, goal half — ES §12.7.2 future
//! reserved words and §13.1.1 `eval` / `arguments`.
//!
//! The parser raised the per-function cases itself
//! (`parser::strict_reserved`); what it could not judge is the GOAL,
//! which `ast.sloppy_script_goal` only carries after the parse. This
//! gate answers that half: under the strict goal every admitted site
//! is a SyntaxError, under the sloppy goal every one of them was an
//! ordinary identifier and nothing moves. Same three-part shape as
//! `yield_ident_goal` and the rotation-372 `delete <bare name>`
//! triage.
//!
//! Both lists share the one site vector: the two are disjoint, so the
//! clause a site belongs to is recoverable from the name itself and
//! nothing has to be tagged at record time.

use super::Ast;

/// `Some(msg)` = strict-goal SyntaxError (the caller reports it as a
/// parse error and stops). Sloppy goal always answers `None`.
pub fn triage_strict_reserved_idents(ast: &Ast) -> Option<String> {
    if ast.sloppy_script_goal {
        return None;
    }
    ast.strict_reserved_positions.first().map(|(at, name)| {
        if name == "eval" || name == "arguments" {
            // One message for both clauses that park this pair: the
            // vector carries no position kind, and §13.1.1 (binding)
            // and §13.1.3 (assignment target) are the only two ways in.
            format!(
                "`{name}` cannot be bound or assigned in strict code \
                 (modules are strict) at {at} (ES §13.1.1 / §13.1.3)"
            )
        } else {
            format!(
                "`{name}` is a reserved word in strict code (modules are strict) at {at} (ES §12.7.2)"
            )
        }
    })
}
