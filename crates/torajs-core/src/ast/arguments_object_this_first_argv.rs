//! The this-first tier's direct-call argv lane — constructors, the
//! receiver-polymorphic static twins, and `this`-reading top-level
//! declarations that read `arguments` at direct calls of different
//! sizes.
//!
//! A this-first body (`__this` first, no `__env`) reading `arguments`
//! sits on the static-argv face, which folds each site's count at
//! compile time and abstains, by construction, the moment the sites
//! disagree or one carries a dynamic spread. What was left holding
//! the count was nothing: the body fell to its declared parameters.
//!
//! ```text
//! var xs = [1, 2, 3];
//! class P { constructor() { … arguments.length … } }
//! class C extends P { constructor() { super(...xs) } }
//! new C()          // 0 before rotation 616's first knife,
//!                  // a loud reject after it, 3 once this face
//!                  // carries the site
//!
//! function t() { return arguments.length }   // `this`-reading body
//! t.call(o, 1, 2); t.call(o, 7)              // 1 and 1 before
//!                                            // rotation 627; 2 and 1
//! ```
//!
//! The shape is the head-less argv face's this-first twin. A body's
//! declaration leads with its head — one slot for a method / twin /
//! promoted declaration (`__this`), two for a constructor (`__this`,
//! `__new_target`) — and "right after the head" is where
//! `inject_argc_params` puts `__torajs_argv`; the SSA side reads the
//! hidden argc slot off the same structural test the `__cm_` method
//! tier uses, and the direct-call terminal packs both slots at that
//! position (`HeadlessSlots::at`). The map's value IS the head width.
//!
//! Admission is as narrow as its siblings', for the same reason: a
//! signature reshape is a memory-safety bug at any call site that
//! does not know about it, so every lane that could reach the body
//! with the old signature must be provably absent.
//!
//! - **the static face did not take it** — a body whose every site
//!   folds statically is already answered, at zero cost; reshaping
//!   it would trade a fold for a runtime channel and gain nothing.
//! - **no default / rest params** — `apply_default_args` and
//!   `apply_rest_args` reshape the same list; keep the two reshapes
//!   from composing.
//! - **every arena `Ident` naming the fn is a call callee** — a
//!   value escape reaches sites that know nothing of the new slot.
//!   For a ctor those Idents are the factory's own call and the
//!   `super(…)` sites; for a twin the `.call` / `.apply`
//!   devirtualizations and the relays; for a promoted declaration
//!   its direct calls and relays — all of which this compiler
//!   rewrites. A value use never names the declaration itself (it
//!   names the `__forward_` relay).
//!
//! **Which one-slot heads.** Only the twins (`ast.smany_twins`) and
//! the promoted declarations (`ast.this_param_fns`). A `__cm_` method
//! is NOT admitted, measured in rotation 616 by lifting the `__ctor`
//! restriction and rebuilding: an admitted `__cm_P__m` reached from
//! `super.m(...xs)` answers `vtable slot 0 of hierarchy \`P\` holds
//! two different signatures`. A method that a `super.m` site can name
//! is by construction on an inheritance chain, and every override
//! sharing a vtable slot has to share a signature — so the whole
//! chain would have to move together or not at all. There was nothing
//! to gain by widening either: an ordinary `recv.m(...xs)` is already
//! answered, because `spread_method_demote` restores the member-call
//! shape and the runtime any-method lane carries the count natively.
//! A twin and a promoted declaration have no vtable slot.

use super::arguments_object_walkers::body_has_any_arguments_touch;
use super::{Ast, Expr, Stmt};

/// Collect the this-first bodies that want a runtime argv channel:
/// name → head width.
pub(super) fn collect_this_first_argv(
    ast: &Ast,
    excluded: &std::collections::HashSet<String>,
    static_argv: &std::collections::HashMap<String, usize>,
) -> std::collections::HashMap<String, usize> {
    let mut out = std::collections::HashMap::new();
    let one_slot: std::collections::HashSet<&String> = ast
        .smany_twins
        .values()
        .chain(ast.this_param_fns.iter())
        .collect();
    for s in &ast.stmts {
        let Stmt::FnDecl {
            name, params, body, ..
        } = s
        else {
            continue;
        };
        let is_ctor = name.starts_with("__cm_") && name.ends_with("__ctor");
        if !is_ctor && !one_slot.contains(name) {
            continue;
        }
        if excluded.contains(name) || static_argv.contains_key(name) {
            continue;
        }
        // The synthetic head slots must be present and in this
        // order — the injection point and the SSA hidden-argc test
        // are both positional. A ctor carries two; the others one.
        if params.first().is_none_or(|p| p.name != "__this") {
            continue;
        }
        let head = if is_ctor {
            if params.get(1).is_none_or(|p| p.name != "__new_target") {
                continue;
            }
            2
        } else {
            1
        };
        if params.iter().any(|p| p.default.is_some() || p.is_rest) {
            continue;
        }
        // Declared parameters are fine on this lane: a direct super
        // site feeds them positionally next to the two slots, and a
        // spread site unpacks them off the materialized list
        // (`lower_this_first_argv_spread`, rotation 625 — the lane used to
        // admit the zero-declared shape only, before that unpack).
        // A ctor that never reads `arguments` still wants the channel
        // when some `super(...xs)` site carries a spread: the runtime
        // width has no other lane to a zero-declared ctor, and the
        // bare fn name died loud at the spread lane's callee boxing
        // (`super(0, ...gen())` on `constructor() {}` — the six
        // test262 `call-spread-err-mult-err-*` cases). Its body does
        // not materialize (`rewrite_one_body` gates that on a touch),
        // so the slots ride along unread.
        if !body_has_any_arguments_touch(ast, body) && !has_spread_call_site(ast, name) {
            continue;
        }
        if !name_only_called_directly(ast, name) {
            continue;
        }
        out.insert(name.clone(), head);
    }
    out
}

/// Some call of `name` carries a `...spread` argument.
fn has_spread_call_site(ast: &Ast, name: &str) -> bool {
    ast.exprs.iter().any(|e| {
        matches!(e, Expr::Call { callee, args }
            if matches!(ast.get_expr(*callee), Expr::Ident(n) if n == name)
                && args.iter().any(|a| matches!(ast.get_expr(*a), Expr::Spread { .. })))
    })
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
