//! A class constructor's runtime argument channel, carried through
//! its factory.
//!
//! `new C(…)` desugars to `__new_C(…)`, and the factory calls
//! `__cm_C__ctor(__this, <new-target>, p0, p1, …)` with exactly the
//! parameters the constructor DECLARES. A constructor on the
//! this-first argc / argv lane (`arguments_object_this_first_argv`)
//! is fed the true count and values at every `super(…)` site, but at
//! the factory's call it saw the declared list instead:
//!
//! ```text
//! class A { constructor(x, y?) { this.n = arguments.length } }
//! new A(1).n      // 2 — the factory passed (x, y)
//! new A(1, 2, 3)  // 2 — and dropped the third value
//! ```
//!
//! §10.2.1.4 / §10.4.4 build the arguments object from the
//! argumentsList of the `new` expression, so the count and the values
//! have to cross the factory. The factory joins the head-less tier
//! (hidden argc at signature position 0, plus the `__torajs_argv`
//! pointer when the constructor reads values): every `new C(…)` site
//! is a direct call that fills both, and the factory's one call to
//! its constructor forwards them unchanged
//! (`ssa_lower_call_terminal_headless::factory_relay_argv`).
//!
//! Admitted only when the constructor body itself touches
//! `arguments` — a constructor on the lane solely because a
//! `super(...xs)` site feeds it keeps a factory with no extra slots,
//! so a class that never reads `arguments` pays nothing at `new` —
//! and only when every reference to the factory is a call callee, the
//! same bar the head-less tier sets for any signature reshape.

use super::arguments_object_headless_argv::name_only_called_directly;
use super::arguments_object_walkers::body_has_any_arguments_touch;
use super::{Ast, Stmt};
use std::collections::{HashMap, HashSet};

/// The head-less tiers with the relaying factories joined: (argv
/// tier, argc tier). The argc tier also takes every argv body, whose
/// materialize reads the hidden count. Asked once the this-first
/// tiers are on `ast`.
pub(super) fn join_factory_relays(
    ast: &Ast,
    headless_argv: HashSet<String>,
    headless_argc: HashSet<String>,
) -> (HashSet<String>, HashSet<String>) {
    let (factory_argc, factory_argv) =
        collect_factory_relays(ast, &ast.this_first_argc_fns, &ast.this_first_argv_fns);
    let argv: HashSet<String> = headless_argv.union(&factory_argv).cloned().collect();
    let argc = headless_argc
        .union(&argv)
        .chain(&factory_argc)
        .cloned()
        .collect();
    (argv, argc)
}

/// The factories that relay a runtime channel: (argc tier, argv
/// subset). A factory is on the argv tier exactly when its
/// constructor is.
fn collect_factory_relays(
    ast: &Ast,
    ctor_argc: &HashMap<String, usize>,
    ctor_argv: &HashMap<String, usize>,
) -> (HashSet<String>, HashSet<String>) {
    let mut argc = HashSet::new();
    let mut argv = HashSet::new();
    for s in &ast.stmts {
        let Stmt::FnDecl { name, body, .. } = s else {
            continue;
        };
        let Some(cname) = name
            .strip_prefix("__cm_")
            .and_then(|r| r.strip_suffix("__ctor"))
        else {
            continue;
        };
        if ctor_argc.get(name) != Some(&2) || !body_has_any_arguments_touch(ast, body) {
            continue;
        }
        // a unit class's factory leads with the evaluation it was
        // called with (`class_unit`), which is not an argument
        if ast.eval_unit_classes.contains(cname) {
            continue;
        }
        let factory = format!("__new_{cname}");
        let admits = ast
            .stmts
            .iter()
            .any(|s| matches!(s, Stmt::FnDecl { name, .. } if *name == factory));
        if !admits || !name_only_called_directly(ast, &factory) {
            continue;
        }
        if ctor_argv.contains_key(name) {
            argv.insert(factory.clone());
        }
        argc.insert(factory);
    }
    (argc, argv)
}
