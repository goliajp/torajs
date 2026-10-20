//! The checker's side of the `then` closure proof: the answer one run
//! takes, and the seed a run hands the next.

use std::cell::OnceCell;
use std::collections::HashMap;

use super::then_opening;
use crate::ast::{Ast, ExprId};
use crate::builtin_proto_shadow::ShadowSet;
use crate::check::Type;

/// The checker's half of the proof, kept on the [`ShadowSet`]
/// the checker carries from run to run.
#[derive(Default)]
pub(crate) struct ThenClosure {
    /// A run before this one, or the mono program after it, found a cast
    /// the proof cannot clear under its types.
    seeded_open: bool,
    /// The proof without types, taken the first time a run asks.
    syntactic: OnceCell<bool>,
}

impl ShadowSet {
    /// Can this program give no object a `then` its type does not show,
    /// as far as the checker can tell inside a run? Proven once per run,
    /// and only when an await of an object asks.
    pub(crate) fn then_closed(&self, ast: &Ast) -> bool {
        let t = &self.then;
        !t.seeded_open
            && *t.syntactic.get_or_init(|| {
                let opening = then_opening(ast, None);
                if std::env::var_os("TORAJS_THEN_CLOSED").is_some() {
                    match &opening {
                        Some(o) => eprintln!("then-closed: open ({} {})", o.why, o.at),
                        None => eprintln!("then-closed: closed"),
                    }
                }
                opening.is_none()
            })
    }

    /// Does a checker run that answered `types` need another run with
    /// the proof marked open? Only when some await was typed by a closed
    /// proof and an `as` cast does not clear under the run's types. Asked
    /// of a run's program and of the mono program after it.
    pub(crate) fn needs_typed_then_run(&self, ast: &Ast, types: &HashMap<ExprId, Type>) -> bool {
        if self.then.seeded_open || !crate::check::awaited::typed_object_await(ast, types) {
            return false;
        }
        let opening = then_opening(ast, Some(types));
        if let Some(o) = &opening
            && std::env::var_os("TORAJS_THEN_CLOSED").is_some()
        {
            eprintln!(
                "then-closed: open under the run's types ({} {})",
                o.why, o.at
            );
        }
        opening.is_some()
    }

    /// Mark the proof open for the next run.
    pub(crate) fn seed_then_open(&mut self) {
        self.then.seeded_open = true;
    }

    /// The seed of a round's first run: the proof open when `open` is (a
    /// round the proof over the mono program asked for).
    pub(crate) fn then_seeded(open: bool) -> ShadowSet {
        let mut s = ShadowSet::default();
        s.then.seeded_open = open;
        s
    }

    /// Carry the run-to-run seed from `prev`.
    pub(crate) fn carry_then_seed(&mut self, prev: &ShadowSet) {
        self.then.seeded_open |= prev.then.seeded_open;
    }
}
