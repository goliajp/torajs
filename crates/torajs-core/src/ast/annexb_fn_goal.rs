//! Annex B function-declaration goal triage — ES §B.3.2 / §B.3.4.
//!
//! `Statement` excludes `Declaration`, so a FunctionDeclaration is
//! not a legal body for an `if` branch or a labelled statement.
//! Annex B hands it back in exactly those two places, and both
//! productions carry the same caveat: "The above rules are only
//! applied when parsing code that is not strict mode code."
//!
//! Two of §11.2.2's three sources of strictness are visible while
//! parsing, and `parser::loops::judge_annexb_fn` refuses those on the
//! spot. The third is the GOAL — a module is strict code — and it is
//! only stamped after the parse (`ast.sloppy_script_goal`, keyed on
//! the `.cts` extension per the bun mapping). So the parser admits
//! the site and records it, and this gate answers the goal half:
//! under a strict goal every recorded site is a SyntaxError, under
//! the sloppy goal every one of them was the Annex B extension doing
//! its job and nothing moves.
//!
//! Same three-part shape as [`super::yield_ident_goal`] and
//! [`super::legacy_octal_sites`].

use super::Ast;

/// `Some(msg)` = strict-goal SyntaxError (the caller reports it as a
/// parse error and stops). Sloppy goal always answers `None`.
pub fn triage_annexb_fn_decls(ast: &Ast) -> Option<String> {
    if ast.sloppy_script_goal {
        return None;
    }
    ast.annexb_fn_positions.first().map(|(at, ctx)| {
        format!(
            "a function declaration is not allowed as the body of {ctx} \
             in strict code (modules are strict) at {at} \
             (ES annexB §B.3.2 / §B.3.4)"
        )
    })
}
