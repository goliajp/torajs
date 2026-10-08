//! A concat past the length bound raises a RangeError on every run.
//! Hoisted, it raises once, on the loop's entry. The two agree unless
//! the loop catches the error and goes on to the next lap, which then
//! must see the error again: a throw check in the body whose throwing
//! arm stays in the loop (a `catch` inside it). Such a loop keeps its
//! concats.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BlockId, Function, IPred, InstKind, Operand, THROW_ACTIVE_SYM, Terminator, ValueId,
};

/// Some read of the pending-throw flag in `body` may lead back into
/// the loop with the throw taken: either a probe whose throwing arm is
/// in `body`, or a read of the flag in a shape that is not a probe.
pub(super) fn catches_inside(func: &Function, body: &HashSet<BlockId>) -> bool {
    let defs = single_defs(func);
    let flag_loads: HashSet<ValueId> = defs
        .iter()
        .filter(|(_, k)| is_flag_load(k, &defs))
        .map(|(v, _)| *v)
        .collect();
    let mut probes = 0usize;
    for b in body {
        let block = &func.blocks[b.0 as usize];
        if let Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk,
            else_blk,
        } = block.term
            && let Some(arm) = defs.get(&c).and_then(|k| throw_arm(k, &flag_loads))
        {
            probes += 1;
            if body.contains(&if arm { then_blk } else { else_blk }) {
                return true;
            }
        }
    }
    // every read of the flag in the loop must be one of the probes seen
    let reads = body
        .iter()
        .flat_map(|b| &func.blocks[b.0 as usize].insts)
        .filter(|i| i.result.is_some_and(|r| flag_loads.contains(&r)))
        .count();
    reads != probes
}

/// Defining inst of every value written exactly once.
fn single_defs(func: &Function) -> HashMap<ValueId, &InstKind> {
    let mut defs: HashMap<ValueId, Option<&InstKind>> = HashMap::default();
    for b in &func.blocks {
        for i in &b.insts {
            if let Some(r) = i.result {
                defs.entry(r)
                    .and_modify(|d| *d = None)
                    .or_insert(Some(&i.kind));
            }
        }
    }
    defs.into_iter()
        .filter_map(|(v, d)| d.map(|k| (v, k)))
        .collect()
}

/// `load (global_ref THROW_ACTIVE)`.
fn is_flag_load(kind: &InstKind, defs: &HashMap<ValueId, &InstKind>) -> bool {
    let InstKind::Load(_, Operand::Value(addr), 0) = kind else {
        return false;
    };
    matches!(defs.get(addr), Some(InstKind::GlobalRef(s)) if s == THROW_ACTIVE_SYM)
}

/// For a probe `icmp ne|eq flag, 0` the arm taken when a throw is
/// pending: `true` for the then arm.
fn throw_arm(cond: &InstKind, flag_loads: &HashSet<ValueId>) -> Option<bool> {
    let InstKind::ICmp(pred, a, b) = cond else {
        return None;
    };
    let flag = |op: &Operand| matches!(op, Operand::Value(v) if flag_loads.contains(v));
    let zero = |op: &Operand| matches!(op, Operand::ConstI64(0));
    if !(flag(a) && zero(b) || zero(a) && flag(b)) {
        return None;
    }
    match pred {
        IPred::Ne => Some(true),
        IPred::Eq => Some(false),
        _ => None,
    }
}
