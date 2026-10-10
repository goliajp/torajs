//! RFC 20260816-headless-argv-face — the head-less tier's runtime
//! argv channel.
//!
//! The argc half landed with RFC 20260810-indirect-argc-abi H1: a
//! head-less top-level fn (no `__env` / `__this` head) that reads
//! `arguments.length` gets a hidden I64 `__torajs_argc` at sig
//! position 0, and every direct-call site prepends the true count.
//! The VALUES had no such channel — a body reaching
//! `arguments[i]` beyond its declared arity materialized
//! `[p0, p1, …]` over the declared params only, so
//!
//! ```text
//! function f(a: number) { … arguments[i] … }
//! f(1, 2, 3)   // arguments.length answered 3 (argc slot works)
//!              // arguments[1] answered undefined  ← silent-wrong
//! ```
//!
//! The static-argv face (`collect_named_static_argv`) covers this
//! shape only when EVERY call site passes the same count; a program
//! with two differently-sized sites falls off it and lands on the
//! declared-params builder above. This module admits those bodies to
//! a real runtime argv channel instead: a synthetic
//! `__torajs_argv: __argvptr()` param at position 0 (the head-less
//! degeneration of the env-first tier's "right after the head"), fed
//! at each direct-call site from a stack buffer the terminal packs.
//!
//! Admission is deliberately narrow — every call site must be a
//! direct call this compiler can rewrite:
//!
//! - head-less (`user_start == 0`) and not shadowed / face-excluded;
//! - the body touches `arguments` in a non-length form (a
//!   length-only body already has everything it needs from the argc
//!   slot);
//! - the body doesn't take the unsafe-return shape the value tier
//!   also refuses (`return arguments[i]` would borrow the array's
//!   stake past its scope drop);
//! - EVERY arena reference to the fn's name is a call callee. A
//!   spread call site is fine: the zero-declared shape is fed
//!   straight off the materialized list
//!   (`try_lower_headless_argv_spread`), and a declared-parameter
//!   shape wraps into its relay (`wrap_dynamic_spread_callees`),
//!   whose adapter enters the body with the true list. A
//!   value escape reaches sites that know nothing of the argv slot;
//!   those escapes are already relayed through `__forward_` shims by
//!   the time this pass runs (pipeline: `synthesize_forwarders` and
//!   friends precede `desugar_arguments_object`), so what remains is
//!   genuinely unrewritable and stays on the old face. The relay
//!   itself is transparent on both of its routes: its boxed adapter
//!   enters the target with the adapter's own argc / argv
//!   (`ssa_lower_boxed_entry::relay`), and its forwarding call is a
//!   direct call like any other.

use super::arguments_object_walkers::{
    body_has_non_length_arguments_touch, body_has_unsafe_return_arguments,
};
use super::{Ast, Expr, Stmt};

/// Collect the head-less argv tier. `static_argv` names are already
/// resolved statically (zero-cost fold) and must not be reshaped;
/// `excluded` / `shadowed` leave every tier.
pub(super) fn collect_headless_argv(
    ast: &Ast,
    shadowed: &std::collections::HashSet<String>,
    excluded: &std::collections::HashSet<String>,
    static_argv: &std::collections::HashMap<String, usize>,
    env_fns: &std::collections::HashSet<String>,
) -> (
    std::collections::HashSet<String>,
    std::collections::HashSet<String>,
) {
    use std::collections::HashSet;
    let mut candidates: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        let Stmt::FnDecl {
            name, params, body, ..
        } = s
        else {
            continue;
        };
        // Head-less means no synthetic head param at all — the
        // env-first and this-first tiers own their own argv lanes.
        if params
            .first()
            .is_some_and(|p| p.name == "__env" || p.name == "__this")
        {
            continue;
        }
        if env_fns.contains(name)
            || shadowed.contains(name)
            || excluded.contains(name)
            || static_argv.contains_key(name)
        {
            continue;
        }
        // A generator's FnDecl is no longer the source body by the
        // time this pass runs — `desugar_generators` (pipeline 215)
        // replaced it with a FACTORY that mints the `__Gen_*` state
        // machine, and the `[...arguments]` it synthesizes into that
        // factory is a machine-field seed, not the user's read.
        //
        // That factory used to be excluded here outright, on the
        // stated ground that "reshaping the factory's params breaks
        // its wiring (`unknown identifier <param>` — the state machine
        // binds positionally)". Rotation 611 tested the claim by
        // lifting the exclusion behind an `if false` and rebuilding:
        // the workspace compiled clean, the conformance gate read
        // 3773/0/4 unchanged, and every generator probe that answered
        // correctly before still did. Nothing binds positionally to
        // what this pass adds — `inject_argc_params` APPENDS hidden
        // slots, it does not reshuffle declared ones.
        //
        // What the exclusion actually bought was silence. A factory
        // outside the argc face is never static-argv-qualified, so its
        // `[...arguments]` takes the documented degrade in
        // `desugar_generators_class` ("a non-qualified one degrades to
        // the declared params") — and a generator declaring no params
        // degrades to an empty argv. `function* g(){ return
        // arguments.length }` called as `g(42, ...[1], ...arr)` then
        // answered 0 where bun answers 4: not a refusal, a wrong
        // answer, which the design principles §4 refuses outright.
        //
        // Admitted, the same program reaches the loud
        // `box_to_any(FnSig)` refusal its function-EXPRESSION twin
        // already had — the two halves of one seam now fail the same
        // way instead of one of them lying. Closing the seam for real
        // needs the `__forward_` relay to carry argv, not just the
        // argc it has carried since RFC 20260815 blade 5; that is an
        // ABI change and has its own RFC. Eleven probes pin the
        // trigger to generator × reads-arguments × spread-call.
        // A `__forward_` relay for this name (a value escape the
        // forwarder passes rerouted) is no bar: the relay's boxed
        // adapter enters the target DIRECTLY with its own argc and
        // argv (`ssa_lower_boxed_entry::relay`), so every any-world
        // call reaches the body with the true list, and the relay's
        // forwarding call — the only route a typed direct call
        // takes — packs exactly its declared arguments, which on
        // that route ARE the whole list. Until rotation 625 such a
        // fn kept the declared-params face and `(g as any)(7)` on
        // `function g() { return arguments[0] }` answered undefined.
        if body_has_non_length_arguments_touch(ast, body) {
            candidates.insert(name.clone());
        }
    }
    if candidates.is_empty() {
        return (candidates.clone(), candidates);
    }
    // Everything that reads argument VALUES is `touched`, admitted or
    // not — the callers need both halves. A declined body keeps the
    // declared-params approximation it had before this face existed,
    // and every lane that would otherwise widen its call sites (the
    // dynamic-spread forwarder wrap) must keep refusing it: turning a
    // loud refusal into a quietly wrong `arguments[i]` is the one
    // outcome worse than not supporting the shape.
    let touched = candidates.clone();
    let mut admitted = candidates;
    admitted.retain(|name| {
        !ast.stmts.iter().any(|s| {
            matches!(s, Stmt::FnDecl { name: n, params, body, .. }
                if n == name
                    // A rest tail leaves the call sites unreadable to
                    // this face: `apply_rest_args` (pipeline 213,
                    // after this pass) bundles the trailing arguments
                    // into ONE array literal, so by lowering time the
                    // terminal sees `f(1, [2, 3])` where the source
                    // wrote `f(1, 2, 3)`.
                    && (params.last().is_some_and(|p| p.is_rest)
                        // A bare `return arguments[i]` would leave the
                        // element borrowing the array's stake past its
                        // scope drop (the value tier refuses it too).
                        || body_has_unsafe_return_arguments(ast, body)))
        })
    });
    admitted.retain(|name| name_only_called_directly(ast, name));
    (admitted, touched)
}

/// Every arena `Ident(name)` occurrence must sit in a call's callee
/// slot, and the name must never appear as a `Closure` literal's
/// target. Conservative by construction: an unrelated binding that
/// happens to share the fn's name fails the walk, which only loses
/// the admission.
pub(super) fn name_only_called_directly(ast: &Ast, name: &str) -> bool {
    use std::collections::HashSet;
    let mut occurrences: HashSet<u32> = HashSet::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        match e {
            Expr::Ident(n) if n == name => {
                occurrences.insert(i as u32);
            }
            // A closure literal over the fn IS a value escape.
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

/// The head-less fns a `__forward_` relay reaches AND the argv face
/// would admit on its own terms. The static face must yield these:
/// a relay's call sites are runtime calls the static count cannot
/// see (its forwarding call does not vote — see
/// `uniform_direct_call_argc`), so a fn with one direct `g0(7)` and
/// an escape `(g0 as any)(9)` folded to the direct site's count and
/// answered the escape's `arguments[0]` undefined. Asked with an
/// empty static map: the question is what the argv face would take
/// if the static face stood aside, which is exactly the decision
/// being made. A fn the argv face would refuse anyway (a rest tail,
/// a bare `return arguments[i]`) stays where it was.
pub(super) fn relayed_argv_fns(
    ast: &Ast,
    shadowed: &std::collections::HashSet<String>,
    excluded: &std::collections::HashSet<String>,
    env_fns: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    let relayed: std::collections::HashSet<&str> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            // A receiver-first relay (`__fwdrecv_`, rotation 649)
            // redirects the same way (`ssa_lower_boxed_entry::relay`);
            // left off this census its target stayed on the static
            // face and the relay's forwarding call packed the
            // declared arity: `[7].map(t2, o)` answered
            // `arguments.length` 1.
            Stmt::FnDecl { name, .. } => name
                .strip_prefix("__forward_")
                .or_else(|| name.strip_prefix("__fwdrecv_")),
            _ => None,
        })
        .collect();
    if relayed.is_empty() {
        return std::collections::HashSet::new();
    }
    let (admitted, _) = collect_headless_argv(
        ast,
        shadowed,
        excluded,
        &std::collections::HashMap::new(),
        env_fns,
    );
    admitted
        .into_iter()
        .filter(|n| relayed.contains(n.as_str()))
        .collect()
}
