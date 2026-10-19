//! Per-ExprId side tables that must ride a specialization clone.
//!
//! `clone_spec_body` deep-clones a generic body into fresh ExprIds;
//! anything the pipeline recorded against the ORIGINAL ids is
//! invisible to the clone unless copied across the id map. Two
//! registries qualify today — both keyed by a call/destructuring
//! site the clone reproduces verbatim:
//!
//! * `ary_destr_groups` — the array-destructuring group registry, so
//!   the specialization's own check picks each group's lane (it can
//!   differ per instantiation: the same `const [a, b] = xs`
//!   destructures an array in one and a generator in the next).
//! * `default_padded_argc` — the source-written argument count of a
//!   call `apply_default_args` padded; without the carry the fresh
//!   ExprId falls back to the padded length and the head-less tier's
//!   `arguments.length` over-counts.
//! * `arity_hole_args` — the `undefined` that same pad wrote into a
//!   slot with no default of its own; the mark is what types it
//!   `Any` instead of `Undefined`.
//! * `field_default_objlits` — the class a nested field-default
//!   literal mints a cell of; without it the specialization's copy
//!   falls back to naming the enclosing factory's class.
//!
//! Split from `check_monomorph.rs` at the 500-line file cap.

use crate::ast::{Ast, ExprId};

/// A demoted speculative `__cm_` rewrite gets the ALT node's shape
/// back at the CALL ExprId, and lowering consults the call id — so
/// the per-call entries the alt node owns move with it.
///
/// * the T-28 pad count — the demoted re-check typed the alt node; a
///   miss leaves `d.m()` with missing any-params reading garbage
///   registers (the struct-method pad miss).
/// * the default pad's source argc — the call's own entry counted
///   the receiver the `__cm_` form passes as its first argument, so
///   `o.m(1)` on `m(a, b = 5)` answered `arguments.length` 2.
pub(crate) fn carry_demoted(
    c: &mut crate::check::Checker,
    owned_ast: &mut Ast,
    call: ExprId,
    alt: ExprId,
) {
    if let Some(n) = c.arity_pad_count.get(&alt).copied() {
        c.arity_pad_count.insert(call, n);
    }
    match owned_ast.default_padded_argc.get(&alt).copied() {
        Some(n) => owned_ast.default_padded_argc.insert(call, n),
        None => owned_ast.default_padded_argc.remove(&call),
    };
}

/// Copy every per-ExprId registry entry from the cloned-from ids to
/// their clones.
pub(crate) fn carry(owned_ast: &mut Ast, id_map: &[(ExprId, ExprId)]) {
    let groups: Vec<(ExprId, i64)> = id_map
        .iter()
        .filter_map(|&(old, new)| {
            owned_ast
                .ary_destr_groups
                .get(&old)
                .map(|&limit| (new, limit))
        })
        .collect();
    owned_ast.ary_destr_groups.extend(groups);
    let argc: Vec<(ExprId, usize)> = id_map
        .iter()
        .filter_map(|&(old, new)| {
            owned_ast
                .default_padded_argc
                .get(&old)
                .map(|&count| (new, count))
        })
        .collect();
    owned_ast.default_padded_argc.extend(argc);
    // `synth_promise_static_calls` — the async desugar's own settle
    // spellings: a cloned async body reproduces those Call sites, and
    // the clone bypasses the Promise-static patch probe as they do.
    let synth: Vec<ExprId> = id_map
        .iter()
        .filter_map(|&(old, new)| {
            owned_ast
                .synth_promise_static_calls
                .contains(&old)
                .then_some(new)
        })
        .collect();
    owned_ast.synth_promise_static_calls.extend(synth);
    carry_awaits(owned_ast, id_map);
    // `arity_hole_args` — the default pad's hole fillers (rotation
    // 515). A cloned body reproduces the padded call verbatim, and a
    // clone whose hole lost the mark types it `Undefined` again,
    // which is the disagreement the mark exists to close.
    let holes: Vec<ExprId> = id_map
        .iter()
        .filter_map(|&(old, new)| owned_ast.arity_hole_args.contains(&old).then_some(new))
        .collect();
    owned_ast.arity_hole_args.extend(holes);
    // `field_default_objlits` — a generic class's specialization
    // reproduces its factory's `__this` seed verbatim, nested field
    // defaults included; a clone that lost the mark stamps the
    // factory's identity on a cell of another class again.
    let owners: Vec<(ExprId, String)> = id_map
        .iter()
        .filter_map(|&(old, new)| {
            owned_ast
                .field_default_objlits
                .get(&old)
                .map(|cname| (new, cname.clone()))
        })
        .collect();
    owned_ast.field_default_objlits.extend(owners);
    carry_optchain(owned_ast, id_map);
}

/// An `await` read and the frame-settled return of a suspending body:
/// a clone that lost the first reads the awaited operand's `value`
/// field, one that lost the second returns the settle call's promise.
pub(crate) fn carry_awaits(owned_ast: &mut Ast, id_map: &[(ExprId, ExprId)]) {
    let marks: Vec<(ExprId, bool)> = (id_map.iter())
        .filter_map(|&(old, new)| {
            let read = owned_ast.await_value_reads.contains(&old);
            let ret = owned_ast.async_ssa.return_calls.contains(&old);
            (read || ret).then_some((new, read))
        })
        .collect();
    for (new, read) in marks {
        if read {
            owned_ast.await_value_reads.insert(new);
        } else {
            owned_ast.async_ssa.return_calls.insert(new);
        }
    }
}

/// `optchain_calls` / `paren_optchain_calls` — the guard of an optional
/// call `a?.m()` (§13.3.9): a clone that lost them lowers a plain call,
/// which throws on a nullish `a`. The base is a node under the call, so
/// the clone is guarded by the cloned base.
fn carry_optchain(owned_ast: &mut Ast, id_map: &[(ExprId, ExprId)]) {
    let map: std::collections::HashMap<ExprId, ExprId> = id_map.iter().copied().collect();
    for &(old, new) in id_map {
        if let Some(base) = owned_ast.optchain_calls.get(&old).copied() {
            let base = map.get(&base).copied().unwrap_or(base);
            owned_ast.optchain_calls.insert(new, base);
        }
        if owned_ast.paren_optchain_calls.contains(&old) {
            owned_ast.paren_optchain_calls.insert(new);
        }
    }
}
