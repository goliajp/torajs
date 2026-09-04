//! Annex B for-in initializer goal triage — ES §B.3.5.
//!
//! §14.7.5's for-in head binds a name and nothing more; Annex B adds
//! `for ( var BindingIdentifier Initializer in Expression ) Statement`,
//! where the initializer is evaluated and assigned once, before the
//! loop, and then overwritten by the first key. It carries the same
//! caveat as the rest of §B.3: "only applied when parsing code that is
//! not strict mode code."
//!
//! Two of §11.2.2's three sources of strictness are visible while
//! parsing and `Parser::try_parse_for_of` refuses those on the spot.
//! The third is the GOAL, stamped only after the parse, so the site
//! parks here and this gate answers that half.
//!
//! Same three-part shape as [`super::annexb_fn_goal`] and
//! [`super::legacy_octal_sites`].

use super::Ast;

/// `Some(msg)` = strict-goal SyntaxError (the caller reports it as a
/// parse error and stops). Sloppy goal always answers `None`.
pub fn triage_annexb_forin_init(ast: &Ast) -> Option<String> {
    if ast.sloppy_script_goal {
        return None;
    }
    ast.annexb_forin_init_positions.first().map(|at| {
        format!(
            "a for-in head may not initialize its binding in strict code \
             (modules are strict) at {at} (ES annexB §B.3.5)"
        )
    })
}
