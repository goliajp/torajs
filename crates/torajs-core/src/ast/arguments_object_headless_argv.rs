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
//!   value escape reaches sites that know nothing of the argv slot;
//!   those escapes are already relayed through `__forward_` shims by
//!   the time this pass runs (pipeline: `synthesize_forwarders` and
//!   friends precede `desugar_arguments_object`), so what remains is
//!   genuinely unrewritable and stays on the old face.

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
        // time this pass runs — `desugar_generators` (pipeline 139)
        // replaced it with a FACTORY that mints the `__Gen_*` state
        // machine, and the `[...arguments]` it synthesizes into that
        // factory is a machine-field seed, not the user's read.
        // Reshaping the factory's params breaks its wiring
        // (`unknown identifier <param>` — the state machine binds
        // positionally). The generator's own `arguments` face is its
        // own gap (plan-state L3b, rotation 415 ③).
        if ast.generator_factory_classes.contains_key(name)
            || ast.async_generator_fns.contains(name)
        {
            continue;
        }
        // A `__forward_` relay already exists for this name — the
        // forwarder passes (pipeline 195, ahead of this one) minted
        // one for a value escape. Its forwarding call carries the
        // relay's RUNTIME argc into the callee's hidden slot, while
        // the terminal's packer can only fill a buffer of the
        // statically written argument count: the body would then
        // materialize argv[0..argc] off the end of a shorter stack
        // slab. Such a fn keeps the declared-params face until the
        // relay can hand its own argv down (registered residue,
        // plan-state L3b).
        let relay = format!("__forward_{name}");
        if ast
            .stmts
            .iter()
            .any(|s| matches!(s, Stmt::FnDecl { name: n, .. } if *n == relay))
        {
            continue;
        }
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
    retain_off_spread_sites(ast, &mut admitted);
    (admitted, touched)
}

/// Drop any candidate with a spread call site. `f(1, ...arr)` passes
/// a count only the runtime knows, and the terminal's packer works
/// from the statically lowered operand list — it would answer the
/// spread's single arena node as one argument. Such calls keep their
/// pre-face behavior (today: a loud refusal at the spread lowering).
/// The runtime-width packing is the follow-up this face's next blade
/// owes (registered residue, plan-state L3b).
fn retain_off_spread_sites(ast: &Ast, candidates: &mut std::collections::HashSet<String>) {
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Ident(n) = ast.get_expr(*callee) else {
            continue;
        };
        if candidates.contains(n)
            && args
                .iter()
                .any(|a| matches!(ast.get_expr(*a), Expr::Spread { .. }))
        {
            let n = n.clone();
            candidates.remove(&n);
        }
    }
}

/// Every arena `Ident(name)` occurrence must sit in a call's callee
/// slot, and the name must never appear as a `Closure` literal's
/// target. Conservative by construction: an unrelated binding that
/// happens to share the fn's name fails the walk, which only loses
/// the admission.
fn name_only_called_directly(ast: &Ast, name: &str) -> bool {
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
