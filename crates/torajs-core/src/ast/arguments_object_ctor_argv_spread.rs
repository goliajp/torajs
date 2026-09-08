//! Constructors join the runtime argv face — the this-first tier's
//! spread lane.
//!
//! A parent constructor that reads `arguments` sits on the
//! static-argv face, which folds each site's count at compile time.
//! That face abstains, by construction, the moment a site carries a
//! dynamic spread (`ctor_uniform_argc` answers `None` on one), and
//! nothing downstream was left holding the count:
//!
//! ```text
//! var xs = [1, 2, 3];
//! class P { constructor() { … arguments.length … } }
//! class C extends P { constructor() { super(...xs) } }
//! new C()          // 0 before rotation 616's first knife,
//!                  // a loud reject after it, 3 once this face
//!                  // carries the site
//! ```
//!
//! The shape is the head-less argv face's this-first twin. A ctor's
//! declaration already leads with two synthetic slots (`__this`,
//! `__new_target`), so "right after the head" means position 2 —
//! `inject_argc_params` puts `__torajs_argv` there, and the SSA side
//! reads the hidden argc slot off the same structural test the
//! `__cm_` method tier uses.
//!
//! Admission is as narrow as its siblings', for the same reason: a
//! signature reshape is a memory-safety bug at any call site that
//! does not know about it, so every lane that could reach the ctor
//! with the old signature must be provably absent.
//!
//! - **the static face did not take it** — a ctor whose every site
//!   folds statically is already answered, at zero cost; reshaping
//!   it would trade a fold for a runtime channel and gain nothing.
//! - **zero declared user parameters** — a declared slot would have
//!   to be read back out of the argv buffer and coerced to its
//!   param type, which is the same blade the head-less face still
//!   owes. `constructor() { … arguments … }` is the shape the
//!   test262 super-call family writes.
//! - **no default / rest params** — `apply_default_args` and
//!   `apply_rest_args` reshape the same list; keep the two reshapes
//!   from composing.
//! **Why constructors only, and not `__cm_` methods generally.**
//! Measured in rotation 616 by lifting the `__ctor` restriction and
//! rebuilding: an admitted `__cm_P__m` reached from `super.m(...xs)`
//! answers
//! `vtable slot 0 of hierarchy \`P\` holds two different signatures`.
//! A method that a `super.m` site can name is by construction on an
//! inheritance chain, and every override sharing a vtable slot has
//! to share a signature — so the whole chain would have to move
//! together or not at all. There was nothing to gain by widening
//! either: an ordinary `recv.m(...xs)` is already answered, because
//! `spread_method_demote` restores the member-call shape and the
//! runtime any-method lane carries the count natively. A
//! constructor has no vtable slot and no demotion path, which is
//! exactly why it was the one left refusing.
//!
//! - **every arena `Ident` naming the ctor is a call callee** — a
//!   value escape reaches sites that know nothing of the new slot.
//!   For a ctor those Idents are the factory's own call and the
//!   `super(…)` sites, both of which this compiler rewrites.

use super::arguments_object_walkers::body_has_any_arguments_touch;
use super::{Ast, Expr, Stmt};

/// Collect the constructors that want a runtime argv channel.
pub(super) fn collect_ctor_argv(
    ast: &Ast,
    excluded: &std::collections::HashSet<String>,
    static_argv: &std::collections::HashMap<String, usize>,
) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    for s in &ast.stmts {
        let Stmt::FnDecl {
            name, params, body, ..
        } = s
        else {
            continue;
        };
        if !(name.starts_with("__cm_") && name.ends_with("__ctor")) {
            continue;
        }
        if excluded.contains(name) || static_argv.contains_key(name) {
            continue;
        }
        // The two synthetic head slots a ctor always carries. Both
        // must be present and in this order — the injection point
        // and the SSA hidden-argc test are both positional.
        if params.first().is_none_or(|p| p.name != "__this")
            || params.get(1).is_none_or(|p| p.name != "__new_target")
        {
            continue;
        }
        if params.iter().any(|p| p.default.is_some() || p.is_rest) {
            continue;
        }
        if params.iter().any(|p| !p.name.starts_with("__")) {
            continue;
        }
        if !body_has_any_arguments_touch(ast, body) {
            continue;
        }
        if !name_only_called_directly(ast, name) {
            continue;
        }
        out.insert(name.clone());
    }
    out
}

/// Every arena `Ident` naming the fn is a call callee (the head-less
/// face's test, asked of a `__cm_` name). A closure literal over the
/// name is a value escape outright.
fn name_only_called_directly(ast: &Ast, name: &str) -> bool {
    use std::collections::HashSet;
    let mut occurrences: HashSet<u32> = HashSet::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        match e {
            Expr::Ident(n) if n == name => {
                occurrences.insert(i as u32);
            }
            Expr::Closure { fn_name, .. } if fn_name == name => return false,
            _ => {}
        }
    }
    let mut callee_slots: HashSet<u32> = HashSet::new();
    for e in &ast.exprs {
        if let Expr::Call { callee, .. } = e
            && occurrences.contains(&callee.0)
        {
            callee_slots.insert(callee.0);
        }
    }
    occurrences.difference(&callee_slots).next().is_none()
}
