//! Where a reader zeroes x21 after taking the pending throw: only at a
//! take x21 may reach as 1. A forward analysis over two states:
//! - `ZERO`: x21 is 0;
//! - `TIED`: x21 is 1 only while the pending-throw flag is set, as
//!   right after a call that may leave it 1 (a signalling function, or
//!   an unguarded reader passing a throw on): such a callee leaves x21
//!   set only with the flag set, and only a take clears the flag. The
//!   flag may already be set at a call (a block that propagates a
//!   throw drops what it holds before it returns); other calls keep
//!   the state, since they preserve x21.
//!
//! A probe of the flag sends `TIED` on as `ZERO` along its continue
//! edge; a join takes `TIED` if either side is; a take leaves `ZERO`,
//! zeroing x21 unless it already was. The throw edge of a probe is
//! zeroed at its target instead when that block is entered from the
//! probe alone, leads to a take and is emitted as code of its own (not
//! hollow, compile/landing.rs): a `finally` entered from both the throw
//! and the continue edge then takes with x21 known 0, and the path
//! that did not throw pays nothing.
//!
//! An unguarded reader starts in `TIED`: its caller may call it with a
//! throw pending (a call the front end judges unable to throw is not
//! probed), and a take reached before any probe then zeroes x21. A
//! normal return may be `TIED` for the same reason; the callers of an
//! unguarded reader count its call as one that may leave x21 set, and
//! the epilogue of a guarded one restores x21.

use torajs_core::ssa::id_hash::{HashMap, HashSet};
use torajs_core::ssa::{
    Block, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
};

use super::landing::Landing;
use super::throw_probe::{flag_refs, targets};
use super::throw_reg::ThrowRegPlan;

const ZERO: u8 = 0;
const TIED: u8 = 1;

/// Where x21 is zeroed: after a take, by (block id, instruction index),
/// and on entry to a block, by block id.
#[derive(Default)]
pub(crate) struct Zeroing {
    pub(crate) takes: HashSet<(u32, usize)>,
    pub(crate) entries: HashSet<u32>,
}

pub(crate) fn plan(
    func: &Function,
    plan: &ThrowRegPlan,
    landing: &Landing,
    guarded: bool,
) -> Zeroing {
    let take_in: Vec<bool> = (func.blocks.iter())
        .map(|b| b.insts.iter().any(|i| plan.is_take(&i.kind)))
        .collect();
    if !take_in.contains(&true) {
        return Zeroing::default();
    }
    let at: HashMap<u32, usize> = (func.blocks.iter().enumerate())
        .map(|(i, b)| (b.id.0, i))
        .collect();
    let succ: Vec<Vec<usize>> = (func.blocks.iter())
        .map(|b| {
            targets(&b.term)
                .iter()
                .filter_map(|t| at.get(&t.0).copied())
                .collect()
        })
        .collect();
    let mut preds = vec![0u32; func.blocks.len()];
    for &k in succ.iter().flatten() {
        preds[k] += 1;
    }
    let takes = reaches(&take_in, &succ);
    let flags = flag_refs(func);
    let probed: Vec<bool> = func
        .blocks
        .iter()
        .map(|b| ends_in_probe(&flags, b))
        .collect();
    // a block of its own on the way to a take, where x21 can be zeroed;
    // not a hollow one, which a branch lands past
    let own = |k: usize| {
        let b = &func.blocks[k];
        preds[k] == 1 && !b.insts.is_empty() && !landing.is_hollow(b.id) && takes[k]
    };
    let mut state_in = vec![ZERO; func.blocks.len()];
    state_in[0] = if guarded { ZERO } else { TIED };
    let mut out = Zeroing::default();
    loop {
        let mut changed = false;
        for (bi, block) in func.blocks.iter().enumerate() {
            let mut state = state_in[bi];
            for (ii, inst) in block.insts.iter().enumerate() {
                if plan.is_take(&inst.kind) {
                    if state != ZERO {
                        out.takes.insert((block.id.0, ii));
                    }
                    state = ZERO;
                } else if plan.may_leave_set(&inst.kind) {
                    state = TIED;
                }
            }
            for (j, &k) in succ[bi].iter().enumerate() {
                // a probe's continue edge (the second) carries ZERO, its
                // throw edge is zeroed at a landing
                let mut edge = if probed[bi] && j == 1 { ZERO } else { state };
                if edge == TIED && probed[bi] && own(k) {
                    out.entries.insert(func.blocks[k].id.0);
                    edge = ZERO;
                }
                if state_in[k] < edge {
                    state_in[k] = edge;
                    changed = true;
                }
            }
        }
        if !changed {
            return out;
        }
    }
}

/// Per block, whether a block marked in `seed` is reachable from it.
fn reaches(seed: &[bool], succ: &[Vec<usize>]) -> Vec<bool> {
    let mut out = seed.to_vec();
    loop {
        let mut changed = false;
        for (k, ss) in succ.iter().enumerate() {
            if !out[k] && ss.iter().any(|&j| out[j]) {
                out[k] = true;
                changed = true;
            }
        }
        if !changed {
            return out;
        }
    }
}

/// Whether the block ends in a probe of the flag its last call left.
fn ends_in_probe(flags: &HashSet<ValueId>, block: &Block) -> bool {
    let since = (block.insts.iter())
        .rposition(|i| matches!(i.kind, InstKind::Call(..) | InstKind::CallIndirect(..)))
        .map_or(0, |k| k + 1);
    matches!(block.term, Terminator::CondBr { cond: Operand::Value(c), .. }
        if probes_flag(flags, &block.insts[since..], c))
}

/// Whether `c` is `icmp ne (load i64 <the flag>), 0` computed in
/// `insts`.
fn probes_flag(flags: &HashSet<ValueId>, insts: &[Inst], c: ValueId) -> bool {
    let def = |v: ValueId| insts.iter().find(|i| i.result == Some(v)).map(|i| &i.kind);
    let Some(InstKind::ICmp(IPred::Ne, Operand::Value(l), Operand::ConstI64(0))) = def(c) else {
        return false;
    };
    matches!(
        def(*l),
        Some(InstKind::Load(Type::I64, Operand::Value(p), 0)) if flags.contains(p)
    )
}
