//! The direct calls the front end leaves without a probe of the flag:
//! it judges them unable to throw (compile/throw_probe.rs
//! `ProbedCalls`). Should one throw after all, its caller runs on with
//! the throw pending, and a callee that leaves x21 set (one that
//! signals, or an unguarded reader passing a throw on) would leave it
//! set there too. That x21 outlives a take in a later callee, which
//! clears the flag under an x21 the callee keeps, and a later call to
//! a signalling function that returns normally then reads as a throw,
//! sending the caller to take a throw already taken.
//!
//! Such a call is harmless when nothing reads x21 after it: the caller
//! makes no call after it, on any path, to a function that may leave
//! x21 set (only after such a call does a probe read x21), and it is
//! a guarded reader, whose epilogue puts back the x21 it found. Else
//! the callee is moved off x21: a signalling one stops signalling, an
//! unguarded reader is guarded. Neither move undoes another, so the
//! roles settle after a few rounds.

use torajs_core::ssa::id_hash::HashMap;
use torajs_core::ssa::{Function, InstKind, Module};

use super::throw_probe::targets;
use super::throw_reg::Role;

/// A direct call not followed by a probe: `funcs[caller]`'s
/// `blocks[block].insts[at]`, calling `funcs[callee]`.
pub(super) struct LaxCall {
    pub(super) caller: usize,
    pub(super) callee: usize,
    pub(super) block: usize,
    pub(super) at: usize,
}

/// Move off x21 each callee of a harmful call in `lax` under `roles`:
/// clear its `signals`, or set its `guarded`. True when one moved.
pub(super) fn settle(
    module: &Module,
    lax: &[LaxCall],
    roles: &[Role],
    signals: &mut [bool],
    guarded: &mut [bool],
) -> bool {
    let leaves = |g: usize| {
        let r = roles[g];
        r.signals || (r.reads && !r.guards)
    };
    // per caller, the blocks from which a call that leaves x21 set is
    // reachable
    let mut later: HashMap<usize, Reach> = HashMap::default();
    let mut moved = false;
    for c in lax.iter().filter(|c| leaves(c.callee)) {
        let f = &module.funcs[c.caller];
        let harmful = !roles[c.caller].guards || {
            let (at, reach) = later
                .entry(c.caller)
                .or_insert_with(|| reaching(f, &leaves));
            let block = &f.blocks[c.block];
            let rest = (block.insts[c.at + 1..].iter()).any(|i| calls(&i.kind, &leaves));
            rest || (targets(&block.term).iter()).any(|t| at.get(&t.0).is_some_and(|&k| reach[k]))
        };
        if harmful {
            if signals[c.callee] {
                signals[c.callee] = false;
            } else {
                guarded[c.callee] = true;
            }
            moved = true;
        }
    }
    moved
}

fn calls(kind: &InstKind, leaves: &impl Fn(usize) -> bool) -> bool {
    matches!(kind, InstKind::Call(g, _) if leaves(g.0 as usize))
}

/// A function's block positions by id, and per block whether a call to
/// a function that may leave x21 set is in it or reachable from it.
type Reach = (HashMap<u32, usize>, Vec<bool>);

fn reaching(f: &Function, leaves: &impl Fn(usize) -> bool) -> Reach {
    let at: HashMap<u32, usize> = (f.blocks.iter().enumerate())
        .map(|(i, b)| (b.id.0, i))
        .collect();
    let succ: Vec<Vec<usize>> = (f.blocks.iter())
        .map(|b| {
            (targets(&b.term).iter())
                .filter_map(|t| at.get(&t.0).copied())
                .collect()
        })
        .collect();
    let mut out: Vec<bool> = (f.blocks.iter())
        .map(|b| b.insts.iter().any(|i| calls(&i.kind, leaves)))
        .collect();
    loop {
        let mut changed = false;
        for (k, ss) in succ.iter().enumerate() {
            if !out[k] && ss.iter().any(|&j| out[j]) {
                out[k] = true;
                changed = true;
            }
        }
        if !changed {
            return (at, out);
        }
    }
}
