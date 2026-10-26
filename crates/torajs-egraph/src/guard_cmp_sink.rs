//! A branch's compare moved past the guard in front of it.
//!
//! ```text
//! A: %c = icmp uge %d, 10            A: (the compare is gone)
//!    br G                               br G
//! G: %o = lshr %v, 49                G: %o = lshr %v, 49
//!    %g = icmp ne %o, 0                 %g = icmp ne %o, 0
//!    cond_br %g, X, B          →        cond_br %g, X, B
//! B: cond_br %c, T, F                B: %c' = icmp uge %d, 10
//!                                       cond_br %c', T, F
//! X: ...                             X: %c = icmp uge %d, 10
//!                                       ...
//! ```
//!
//! Codegen fuses a compare into the branch it feeds only when the
//! compare is the last instruction of the branch's block and that
//! branch is its one reader (`brfuse`). An integer loop's growth guard
//! (`int_entry`) sits between the exit test and its branch, so the test
//! paid `cset` into a register and `cbnz` on it on every iteration;
//! the guard's cold side, which re-enters the f64 copy of the loop,
//! reads the same flag. Here the compare moves into `B`, under a fresh
//! value when anything else reads it, and is recomputed at the head of
//! `X` when the cold side still reads it there. Both copies read the
//! same operands: nothing between `A` and either block redefines them,
//! and `B` and `X` are the only ways on from `G`.
//!
//! `TORAJS_GUARD_CMP_SINK_OFF=1` skips.

use torajs_core::ssa::{
    FPred, Function, Inst, InstKind, Module, Operand, Terminator, ValueId, visit_value_operands,
};

use crate::float_demote::push_value;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GuardCmpSinkStats {
    /// Compares moved into the block whose branch reads them.
    pub sunk: u32,
    /// Of those, the ones recomputed on the guard's other side too.
    pub recomputed: u32,
}

/// How far the liveness walk goes before it gives up and calls the
/// value live.
const WALK_LIMIT: usize = 64;

pub fn sink_guarded_cmps(module: &mut Module) -> GuardCmpSinkStats {
    let mut stats = GuardCmpSinkStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() || !has_candidate(func) {
            continue;
        }
        let preds = reachable_preds(func);
        for b in 0..func.blocks.len() {
            sink_one(func, b, &preds, &mut stats);
        }
    }
    stats
}

/// Some block branches on a value it does not define and is entered
/// from a conditional branch: most functions have none.
fn has_candidate(func: &Function) -> bool {
    let n = func.blocks.len();
    let branches_on_outside = |b: usize| {
        b < n
            && matches!(&func.blocks[b].term, Terminator::CondBr { cond: Operand::Value(c), .. }
                if !func.blocks[b].insts.iter().any(|i| i.result == Some(*c)))
    };
    func.blocks.iter().any(|g| {
        matches!(g.term, Terminator::CondBr { then_blk, else_blk, .. }
            if branches_on_outside(then_blk.0 as usize) || branches_on_outside(else_blk.0 as usize))
    })
}

/// A block's predecessors among the blocks reachable from the entry,
/// as far as this pass asks: none, exactly one, or more.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Preds {
    None,
    One(usize),
    Many,
}

/// Predecessors over the blocks reachable from the entry: a block a
/// conversion emptied still branches on to its old successor.
fn reachable_preds(func: &Function) -> Vec<Preds> {
    let n = func.blocks.len();
    let mut live = vec![false; n];
    let mut work = vec![0usize];
    let mut preds = vec![Preds::None; n];
    while let Some(b) = work.pop() {
        if b >= n || std::mem::replace(&mut live[b], true) {
            continue;
        }
        let (ts, k) = succs(&func.blocks[b].term);
        for &s in &ts[..k] {
            if s < n {
                preds[s] = match preds[s] {
                    Preds::None => Preds::One(b),
                    Preds::One(p) if p == b => Preds::One(p),
                    _ => Preds::Many,
                };
            }
            work.push(s);
        }
    }
    preds
}

/// The successors, and how many of the two slots hold one.
fn succs(term: &Terminator) -> ([usize; 2], usize) {
    match term {
        Terminator::Br(t) => ([t.0 as usize, 0], 1),
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => ([then_blk.0 as usize, else_blk.0 as usize], 2),
        Terminator::Ret(_) | Terminator::Unreachable => ([0, 0], 0),
    }
}

fn sole(preds: &[Preds], b: usize) -> Option<usize> {
    match preds[b] {
        Preds::One(p) => Some(p),
        _ => None,
    }
}

fn reads(inst: &Inst, v: ValueId) -> bool {
    let mut hit = false;
    visit_value_operands(&inst.kind, |x| hit |= x == v);
    hit
}

fn term_reads(term: &Terminator, v: ValueId) -> bool {
    matches!(term, Terminator::CondBr { cond: Operand::Value(c), .. } | Terminator::Ret(Some(Operand::Value(c))) if *c == v)
}

/// `insts` neither reads `c` nor defines it or one of `pinned`.
fn clear(insts: &[Inst], c: ValueId, pinned: &[ValueId]) -> bool {
    insts
        .iter()
        .all(|i| !reads(i, c) && i.result.is_none_or(|r| r != c && !pinned.contains(&r)))
}

fn sink_one(func: &mut Function, b: usize, preds: &[Preds], stats: &mut GuardCmpSinkStats) {
    let Terminator::CondBr {
        cond: Operand::Value(c),
        ..
    } = func.blocks[b].term
    else {
        return;
    };
    let Some(g) = sole(preds, b) else { return };
    let Terminator::CondBr {
        cond,
        then_blk,
        else_blk,
    } = func.blocks[g].term
    else {
        return;
    };
    let x = if then_blk.0 as usize == b {
        else_blk.0 as usize
    } else {
        then_blk.0 as usize
    };
    if g == b || x == b || cond == Operand::Value(c) || sole(preds, x) != Some(g) {
        return;
    }
    // the compare: last def of `c` in G, or in G's one predecessor
    // when that falls through into G
    let last_def = |blk: usize| {
        func.blocks[blk]
            .insts
            .iter()
            .rposition(|i| i.result == Some(c))
    };
    let (a, at) = match last_def(g) {
        Some(at) => (g, at),
        None => {
            let Some(a) = sole(preds, g) else { return };
            if a == g || !matches!(func.blocks[a].term, Terminator::Br(t) if t.0 as usize == g) {
                return;
            }
            let Some(at) = last_def(a) else { return };
            (a, at)
        }
    };
    let cmp = func.blocks[a].insts[at].clone();
    let pinned: Vec<ValueId> = match &cmp.kind {
        InstKind::ICmp(_, l, r) => [l, r],
        InstKind::FCmp(p, l, r) if *p != FPred::One => [l, r],
        _ => return,
    }
    .into_iter()
    .filter_map(|o| match o {
        Operand::Value(v) => Some(*v),
        _ => None,
    })
    .collect();
    let mut gap: Vec<&[Inst]> = vec![&func.blocks[a].insts[at + 1..], &func.blocks[b].insts];
    if a != g {
        gap.push(&func.blocks[g].insts);
    }
    if !gap.iter().all(|s| clear(s, c, &pinned)) {
        return;
    }
    let (after, k) = succs(&func.blocks[b].term);
    if live_from(func, &after[..k], c) {
        return;
    }
    let other_side = live_from(func, &[x], c);
    let ty = func.value_type(c);
    func.blocks[a].insts.remove(at);
    let fresh = push_value(&mut func.values, ty);
    let mut moved = cmp.clone();
    moved.result = Some(fresh);
    func.blocks[b].insts.push(moved);
    if let Terminator::CondBr { cond, .. } = &mut func.blocks[b].term {
        *cond = Operand::Value(fresh);
    }
    stats.sunk += 1;
    if other_side {
        func.blocks[x].insts.insert(0, cmp);
        stats.recomputed += 1;
    }
}

/// Whether some path from the entries of `starts` reads `v` before
/// redefining it. A walk past [`WALK_LIMIT`] blocks answers yes.
fn live_from(func: &Function, starts: &[usize], v: ValueId) -> bool {
    let n = func.blocks.len();
    let mut seen = vec![false; n];
    let mut work: Vec<usize> = starts.to_vec();
    let mut steps = 0;
    while let Some(b) = work.pop() {
        if b >= n || std::mem::replace(&mut seen[b], true) {
            continue;
        }
        steps += 1;
        if steps > WALK_LIMIT {
            return true;
        }
        let block = &func.blocks[b];
        let mut killed = false;
        for inst in &block.insts {
            if reads(inst, v) {
                return true;
            }
            if inst.result == Some(v) {
                killed = true;
                break;
            }
        }
        if killed {
            continue;
        }
        if term_reads(&block.term, v) {
            return true;
        }
        let (next, k) = succs(&block.term);
        work.extend_from_slice(&next[..k]);
    }
    false
}

#[cfg(test)]
mod tests;
