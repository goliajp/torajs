//! Where an operand's integer form may be computed other than at the
//! `frem`.
//!
//! A loop counter is a multi-def cell, so the `sitofp` that feeds the
//! `frem` is trusted only while nothing redefines the counter between
//! the two; [`stable_chain`] follows the single-predecessor edges back
//! from the `frem` to the `sitofp` to show that.
//!
//! A divisor read in a loop but defined outside every loop (`i % pool`
//! with `pool` from `parseInt`) is converted and checked once, right
//! after its definition, instead of on every iteration: [`hoist`].

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{Function, Inst, InstKind, Operand, Terminator, Type, ValueId};

use super::lane::{Lane, Role};
use crate::float_demote::push_value;

/// Values defined exactly once, a parameter counting as its def.
pub(super) fn fixed_values(func: &Function) -> HashSet<ValueId> {
    let mut count: HashMap<ValueId, u32> = func.params.iter().map(|p| (*p, 1)).collect();
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        if let Some(r) = inst.result {
            *count.entry(r).or_default() += 1;
        }
    }
    count
        .into_iter()
        .filter_map(|(v, n)| (n == 1).then_some(v))
        .collect()
}

fn succs(term: &Terminator) -> Vec<usize> {
    match term {
        Terminator::Br(t) => vec![t.0 as usize],
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => vec![then_blk.0 as usize, else_blk.0 as usize],
        Terminator::Ret(_) | Terminator::Unreachable => vec![],
    }
}

/// Some path leaves `b` and comes back to it.
fn on_cycle(func: &Function, b: usize) -> bool {
    let mut seen = vec![false; func.blocks.len()];
    let mut stack = succs(&func.blocks[b].term);
    while let Some(n) = stack.pop() {
        if n == b {
            return true;
        }
        if !std::mem::replace(&mut seen[n], true) {
            stack.extend(succs(&func.blocks[n].term));
        }
    }
    false
}

fn defines(insts: &[Inst], x: ValueId) -> bool {
    insts.iter().any(|i| i.result == Some(x))
}

/// `x` still holds what `v = sitofp x` read when the `frem` at
/// `(bi, at)` runs: walking back through blocks with exactly one
/// predecessor reaches the `sitofp` with no def of `x` on the way.
pub(super) fn stable_chain(func: &Function, bi: usize, at: usize, v: ValueId, x: ValueId) -> bool {
    if defines(&func.blocks[bi].insts[..at], x) {
        return false;
    }
    let mut cur = bi;
    for _ in 0..8 {
        // the entry block is also reached from outside the function
        if cur == 0 {
            return false;
        }
        let mut preds =
            (0..func.blocks.len()).filter(|&p| succs(&func.blocks[p].term).contains(&cur));
        let (Some(p), None) = (preds.next(), preds.next()) else {
            return false;
        };
        let insts = &func.blocks[p].insts;
        if let Some(pos) = insts.iter().position(|i| i.result == Some(v)) {
            return !defines(&insts[pos + 1..], x);
        }
        if defines(insts, x) || p == bi {
            return false;
        }
        cur = p;
    }
    false
}

/// Where a single-def value is defined: block and the index right
/// after the def; a parameter is defined on entry.
fn def_point(func: &Function, a: ValueId) -> Option<(usize, usize)> {
    if func.params.contains(&a) {
        return Some((0, 0));
    }
    func.blocks.iter().enumerate().find_map(|(bi, b)| {
        let at = b.insts.iter().position(|i| i.result == Some(a))?;
        Some((bi, at + 1))
    })
}

/// Lower a truncated operand once, after its definition, when the
/// `frem` in block `bi` sits on a cycle and the definition does not;
/// `cache` shares the lowering among every `frem` that reads it.
pub(super) fn hoist(
    func: &mut Function,
    bi: usize,
    lane: Lane,
    role: Role,
    fixed: &HashSet<ValueId>,
    cache: &mut HashMap<(ValueId, Role), Lane>,
) -> Lane {
    let Lane::Float { a, .. } = lane else {
        return lane;
    };
    if let Some(ready) = cache.get(&(a, role)) {
        return *ready;
    }
    if !fixed.contains(&a) {
        return lane;
    }
    let Some((db, at)) = def_point(func, a) else {
        return lane;
    };
    if db == bi || on_cycle(func, db) || !on_cycle(func, bi) {
        return lane;
    }
    let mut new = Vec::new();
    let values = &mut func.values;
    let mut emit = |ty: Type, kind: InstKind| {
        let result = push_value(values, ty);
        new.push(Inst {
            result: Some(result),
            kind,
            origin: None,
        });
        Operand::Value(result)
    };
    let (t, check) = lane.lower(role, &mut emit);
    func.blocks[db].insts.splice(at..at, new);
    let ready = Lane::Ready { t, check };
    cache.insert((a, role), ready);
    ready
}
