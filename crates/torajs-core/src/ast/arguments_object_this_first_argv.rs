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
//! The shape is the head-less face's this-first twin, in both of its
//! tiers. A body's declaration leads with its head — one slot for a
//! method / twin / promoted declaration (`__this`), two for a
//! constructor (`__this`, `__new_target`) — and "right after the
//! head" is where the hidden I64 argc sits in the SSA signature
//! (`ssa_lower_pass_1::this_first_hidden_argc_pos`, keyed on
//! `Ast::this_first_argc_fns`) and, for a body that reads argument
//! VALUES, where `inject_argc_params` puts `__torajs_argv` right
//! behind it; the direct-call terminal inserts the slots a callee has
//! at that position (`HeadlessSlots::at`). The maps' value IS the
//! head width.
//!
//! **Two tiers, as the head-less face has them** (rotation 650). A
//! body that reads only `arguments.length` is on the argc tier alone
//! (`this_first_argc_fns`); one that reads values — or that never
//! reads `arguments` at all but has a `...spread` site feeding it,
//! the ctor shape below — is on both (`this_first_argv_fns` ⊆
//! `this_first_argc_fns`). Before the split every admitted body took
//! the argv channel, and a length-only body then materialized its
//! `arguments` off the buffer regardless — so a relay's forwarding
//! call, which packs exactly its declared slots, had to cap the
//! count it forwards at that pack (`forwarded_argc`), and
//! `[7].map(t, o)` on a `this`-reading `t` that reads only the count
//! answered 1 where bun says 3. Lifting the cap alone died on the
//! over-read; an argc-only body has no buffer to over-read.
//!
//! Admission is as narrow as its siblings', for the same reason: a
//! signature reshape is a memory-safety bug at any call site that
//! does not know about it, so every lane that could reach the body
//! with the old signature must be provably absent.
//!
//! - **the static face did not take it** — a body whose every site
//!   folds statically is already answered, at zero cost; reshaping
//!   it would trade a fold for a runtime channel and gain nothing.
//! - **no rest param** — `apply_rest_args` bundles the trailing
//!   arguments into one array, so the sites no longer spell the
//!   count. Defaults are kept off the one-slot heads, whose sites
//!   were never audited against the padding; a constructor takes
//!   them, since `apply_default_args` pads past the injected argv
//!   pointer (`collect_fn_defaults`) and the terminal counts the
//!   source-written arguments (`default_padded_argc`).
//! - **every arena `Ident` naming the fn is a call callee** — a
//!   value escape reaches sites that know nothing of the new slot.
//!   For a ctor those Idents are the factory's own call and the
//!   `super(…)` sites; for a twin the `.call` / `.apply`
//!   devirtualizations and the relays; for a promoted declaration
//!   its direct calls and relays — all of which this compiler
//!   rewrites. A value use never names the declaration itself (it
//!   names the `__forward_` relay).
//!
//! **Which one-slot heads.** The twins (`ast.smany_twins`), the
//! promoted declarations (`ast.this_param_fns`), and — on the argc
//! tier only — a single-owner `__cm_` method whose body reads nothing
//! but `arguments.length` ([`method_admits`]). A method that some
//! `super.m` / vtable / `__dispatch_` lane can name is excluded: every
//! override sharing a vtable slot has to share a signature (admitting
//! one answers `vtable slot 0 of hierarchy \`P\` holds two different
//! signatures`), so the whole slot moves together: every row and the
//! `__dispatch_` stub of a slot some row reads the count in
//! ([`slot::collect_slot_argc`]). A method is also excluded when
//! lowering calls it by its spelled name rather than through the
//! direct-call terminal or the boxed adapter (the iterator protocol,
//! `toJSON`, `then`, class accessors). Argc lives in the SSA
//! signature only, so a defaulted or optional parameter composes:
//! the terminal answers the count the source wrote
//! (`default_padded_argc`), not the padded one.

mod method;
mod slot;
use super::arguments_object_walkers::{
    body_has_any_arguments_touch, body_has_non_length_arguments_touch,
};
use super::{Ast, Expr, Stmt};
use method::collect_method_argc;

/// Collect the this-first bodies that want a runtime argc channel
/// (`argc`, name → head width) and the subset that also wants the
/// argv channel (`argv`). A length-only body with no spread site
/// is in `argc` alone.
pub(super) fn collect_this_first_argv(
    ast: &Ast,
    excluded: &std::collections::HashSet<String>,
    static_argv: &std::collections::HashMap<String, usize>,
    slot_argc: &[String],
) -> ThisFirstTiers {
    let mut out = ThisFirstTiers::default();
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
        let Some(head) = head_width(params, is_ctor) else {
            continue;
        };
        if params
            .iter()
            .any(|p| p.is_rest || p.default.is_some() && !is_ctor)
        {
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
        let spread_site = has_spread_call_site(ast, name);
        if !body_has_any_arguments_touch(ast, body) && !spread_site {
            continue;
        }
        if !name_only_called_directly(ast, name) {
            continue;
        }
        out.argc.insert(name.clone(), head);
        // A spread site feeds the declared parameters off the
        // materialized list (`lower_this_first_argv_spread`), so it
        // needs the argv channel even for a length-only body.
        if spread_site || body_has_non_length_arguments_touch(ast, body) {
            out.argv.insert(name.clone(), head);
        }
    }
    out.argc.extend(
        collect_method_argc(ast, excluded, static_argv)
            .into_iter()
            .chain(slot_argc.iter().cloned())
            .map(|n| (n, 1)),
    );
    out
}

/// The head a this-first declaration carries. The synthetic head
/// slots must be present and in this order — the injection point and
/// the SSA hidden-argc test are both positional. A ctor carries two;
/// the others one, except that a plain function reading `new.target`
/// carries the ctor's two slots too (`new_target_param.rs`).
fn head_width(params: &[super::Param], is_ctor: bool) -> Option<usize> {
    if params.first().is_none_or(|p| p.name != "__this") {
        return None;
    }
    if params.get(1).is_some_and(|p| p.name == "__new_target") {
        Some(2)
    } else if is_ctor {
        None
    } else {
        Some(1)
    }
}

/// The two this-first tiers `collect_this_first_argv` answers.
#[derive(Default)]
pub(super) struct ThisFirstTiers {
    /// Every admitted body: hidden argc right after the head.
    pub(super) argc: std::collections::HashMap<String, usize>,
    /// The subset that also takes `__torajs_argv` behind the argc.
    pub(super) argv: std::collections::HashMap<String, usize>,
}

/// The this-first bodies a `__forward_` / `__fwdrecv_` relay reaches
/// AND this face would admit on its own terms — the static face
/// must yield these, exactly as it yields the head-less ones
/// (`arguments_object_headless_argv::relayed_argv_fns`): a relay's
/// calls are runtime calls the static count cannot see, so a
/// `this`-reading declaration with one direct `t.call(o, 1, 2, 3, 4)`
/// and a callback use `xs.map(t, o)` folded to the direct site's
/// count and answered the callback's `arguments.length` 4 and
/// `arguments[1]` undefined (rotation 650). Asked with an empty
/// static map: the question is what this face would take if the
/// static face stood aside, which is exactly the decision being
/// made.
///
/// The rows and stubs of every vtable slot this face admits yield
/// too: a row is reached through the slot as well as by name, and a
/// `super.m(x)` site is the only one the fold sees.
pub(super) fn relayed_this_first_fns(
    ast: &Ast,
    excluded: &std::collections::HashSet<String>,
    slot_argc: &[String],
) -> std::collections::HashSet<String> {
    let mut out: std::collections::HashSet<String> = slot_argc.iter().cloned().collect();
    let relayed: std::collections::HashSet<&str> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } => name
                .strip_prefix("__forward_")
                .or_else(|| name.strip_prefix("__fwdrecv_")),
            _ => None,
        })
        .collect();
    if relayed.is_empty() {
        return out;
    }
    out.extend(
        collect_this_first_argv(ast, excluded, &std::collections::HashMap::new(), slot_argc)
            .argc
            .into_keys()
            .filter(|n| relayed.contains(n.as_str())),
    );
    out
}

/// The rows and stubs of every vtable slot the argc tier admits,
/// computed once per program for both askers above.
pub(super) fn slot_argc_fns(
    ast: &Ast,
    excluded: &std::collections::HashSet<String>,
) -> Vec<String> {
    slot::collect_slot_argc(ast, excluded)
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
