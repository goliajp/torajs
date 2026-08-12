//! The names strict mode code refuses, and where it refuses them.
//!
//! Two lists, because they are refused in different positions:
//!
//! * **§12.7.2 future reserved words** — `implements interface package
//!   private protected public static`. Reserved in EVERY `Identifier`
//!   occurrence of strict code, reference positions included. (`let`
//!   and `yield` belong to the same list but reach the parser as their
//!   own tokens; `yield` keeps the dedicated `yield_ident_positions`
//!   lane it got in rotation 374.)
//! * **§13.1.1 `eval` / `arguments`** — refused only where strict code
//!   would BIND or ASSIGN them. Reading them stays legal everywhere:
//!   `typeof eval` is fine in the strictest module there is, which is
//!   why this half must never be folded into the reference judge.
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

/// §13.1.1 — refused in binding and assignment positions of strict
/// code, admitted everywhere else.
const STRICT_BAD_BINDING: [&str; 2] = ["eval", "arguments"];

/// Whether `name` is refused where strict code binds or assigns it —
/// the union of both lists, since a future reserved word is refused in
/// every position including these.
pub(super) fn refused_as_binding(name: &str) -> bool {
    STRICT_RESERVED.contains(&name) || STRICT_BAD_BINDING.contains(&name)
}

impl Parser<'_> {
    /// Judge `name` in a BindingIdentifier position. `Err` when the
    /// enclosing function is already known to be strict; otherwise the
    /// site is recorded for the goal gate and `Ok` admits it.
    pub(super) fn note_strict_binding(&mut self, name: &str) -> Result<(), String> {
        if !refused_as_binding(name) {
            return Ok(());
        }
        self.reject_if_strict_binding(name, self.in_strict_fn)?;
        self.record_strict_goal_site(name);
        Ok(())
    }

    /// Park `name` for the goal gate, for callers that have already
    /// answered the per-function half themselves.
    pub(super) fn record_strict_goal_site(&mut self, name: &str) {
        let at = self.at();
        self.ast
            .strict_reserved_positions
            .push((at, name.to_string()));
    }

    /// Binding-position judge, with the verdict passed IN rather than
    /// read off `self`. The parameter-list caller has already restored
    /// the enclosing function's bit by the time it asks — its own
    /// strictness came from the body that has just finished parsing —
    /// so consulting `self.in_strict_fn` here would answer for the
    /// wrong function.
    pub(super) fn reject_if_strict_binding(&self, name: &str, strict: bool) -> Result<(), String> {
        if strict && STRICT_BAD_BINDING.contains(&name) {
            return Err(format!(
                "`{name}` cannot be bound in strict code at {} (ES §13.1.1)",
                self.at()
            ));
        }
        self.reject_if_strict_reserved(name, strict)
    }

    /// Reference-position judge — the §12.7.2 half only. `eval` and
    /// `arguments` stay readable in strict code, so they are absent
    /// here by design.
    pub(super) fn reject_if_strict_reserved(&self, name: &str, strict: bool) -> Result<(), String> {
        if strict && STRICT_RESERVED.contains(&name) {
            return Err(format!(
                "`{name}` is a reserved word in strict code at {} (ES §12.7.2)",
                self.at()
            ));
        }
        Ok(())
    }
}
