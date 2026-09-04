//! Duplicate-parameter goal triage — ES §15.1.2.
//!
//! `FormalParameters` may repeat a name; `UniqueFormalParameters` may
//! not. The difference is what a method or an arrow takes versus what
//! a function declaration or expression takes — and on top of it
//! §15.1.2 adds one more condition to the permissive form: duplicates
//! are a Syntax Error when the parameter list is strict mode code, or
//! when the list is not simple.
//!
//! `parser::param_list_early_errors` answers the goal-independent
//! halves where the names are read, and
//! `parser::strict_directive::judge_duplicate_params_strict` answers
//! the two strictness sources visible while parsing, at the end of
//! the body (a function's own `"use strict"` sits inside the body its
//! parameters precede). This gate answers the third: a module is
//! strict code, and that is stamped only after the parse.
//!
//! Same three-part shape as [`super::annexb_fn_goal`] and
//! [`super::legacy_octal_sites`].

use super::Ast;

/// `Some(msg)` = strict-goal SyntaxError (the caller reports it as a
/// parse error and stops). Sloppy goal always answers `None`.
pub fn triage_duplicate_params(ast: &Ast) -> Option<String> {
    if ast.sloppy_script_goal {
        return None;
    }
    ast.dup_param_positions.first().map(|(at, name)| {
        format!(
            "duplicate parameter name `{name}` is not allowed in strict code \
             (modules are strict) at {at} (ES §15.1.2)"
        )
    })
}
