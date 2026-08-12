//! §12.7.2 — the future reserved words that are reserved ONLY in
//! strict mode code: `implements interface package private protected
//! public static`. (`let` and `yield` belong to the same list but
//! reach the parser as their own tokens; `yield` keeps the dedicated
//! `yield_ident_positions` lane it got in rotation 374.)
//!
//! Two answers, because strictness arrives from two directions:
//!
//! * **Per-function** — the enclosing function said `"use strict"`,
//!   possibly several levels up (`parser::strict_directive`). The
//!   parser knows this while it parses, so the SyntaxError is raised
//!   on the spot.
//! * **Per-goal** — module code is strict (§16.1), but the goal bit is
//!   stamped AFTER parsing. So the site is admitted and recorded, and
//!   the prelude gate `triage_strict_reserved_idents` raises it, the
//!   same shape `yield` and `delete <bare name>` already use.
//!
//! Sloppy script code keeps every one of these as an ordinary
//! identifier, which is why the default has to be admission.

use super::*;

/// The strict-only future reserved words that arrive as plain
/// identifiers.
const STRICT_RESERVED: [&str; 7] = [
    "implements",
    "interface",
    "package",
    "private",
    "protected",
    "public",
    "static",
];

impl Parser<'_> {
    /// Judge `name` in a BindingIdentifier position. `Err` when the
    /// enclosing function is already known to be strict; otherwise the
    /// site is recorded for the goal gate and `Ok` admits it.
    pub(super) fn note_strict_reserved_binding(&mut self, name: &str) -> Result<(), String> {
        if !STRICT_RESERVED.contains(&name) {
            return Ok(());
        }
        if self.in_strict_fn {
            return Err(format!(
                "`{name}` is a reserved word in strict code at {} (ES §12.7.2)",
                self.at()
            ));
        }
        let at = self.at();
        self.ast
            .strict_reserved_positions
            .push((at, name.to_string()));
        Ok(())
    }
}
