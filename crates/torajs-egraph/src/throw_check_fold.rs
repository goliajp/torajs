//! Throw-check folding — drop the `___torajs_throw_active` probes
//! whose answer is already known to be "nothing pending".
//!
//! Lowering follows every call that may throw with a probe of the
//! pending-exception flag (`ssa_lower_emit_throw_check`):
//!
//! ```text
//! %p = global_ref @___torajs_throw_active
//! %v = load i64, %p
//! %c = icmp ne %v, 0
//! cond_br %c, <propagate>, <continue>
//! ```
//!
//! The flag changes only inside a call: the runtime writes it in
//! `__torajs_throw_set` / `__torajs_throw_take`, and SSA code never
//! stores to it. So on the `<continue>` edge of a probe the flag is 0,
//! and it stays 0 until the next `Call` / `CallIndirect`. Once the
//! inliner has spliced a callee with no call of its own into a loop,
//! the probe that followed the call is dominated by an earlier probe
//! with no call in between — closure-counter's loop read the flag
//! twice per iteration for one possible answer.
//!
//! This is a forward must-analysis over {known zero, unknown}: a
//! call makes the flag unknown, a probe's `<continue>` edge makes it
//! known zero, a join is known zero only if every predecessor says
//! so, and the entry is unknown. A probe reached in the known-zero
//! state branches straight to `<continue>`; its `<propagate>` block
//! loses that predecessor and the unreachable-block sweep takes it.
//! Nothing here assumes that every throwing call is followed by a
//! probe: a call the lowering left unprobed is still a call, and
//! still makes the flag unknown (the P1b design report, route A).
//!
//! Runs after `branch_fold`: an array element read's out-of-range arm
//! holds the call that raises the RangeError, and the probe after the
//! read is foldable only once that arm has been folded away.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    BlockId, Function, IPred, InstKind, Module, Operand, THROW_ACTIVE_SYM, Terminator, Type,
    ValueId, visit_value_operands,
};

use crate::branch_fold::sweep_unreachable_blocks;
use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::{LoopAnalysis, NaturalLoop};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ThrowCheckFoldStats {
    pub probes: u32,
    pub folded: u32,
    pub blocks_removed: u32,
    pub loop_entry_probes: u32,
}

pub fn fold_throw_checks(module: &mut Module) -> ThrowCheckFoldStats {
    let mut stats = ThrowCheckFoldStats::default();
    for func in module.funcs.iter_mut() {
        fold_in_function(func, &mut stats);
    }
    stats
}

/// A block ending in a flag probe: the flag's load sits at `load` and
/// no call follows it in the block, so the else edge means "zero".
struct Probe {
    load: usize,
    values: [ValueId; 3],
    then_blk: BlockId,
    else_blk: BlockId,
}

fn fold_in_function(func: &mut Function, stats: &mut ThrowCheckFoldStats) {
    fold_known_zero(func, stats);
    let guarded = guard_loop_entries(func);
    if guarded > 0 {
        stats.loop_entry_probes += guarded;
        fold_known_zero(func, stats);
    }
}

fn fold_known_zero(func: &mut Function, stats: &mut ThrowCheckFoldStats) {
    let probes: HashMap<usize, Probe> = (0..func.blocks.len())
        .filter_map(|b| probe_of(func, b).map(|p| (b, p)))
        .collect();
    if probes.is_empty() {
        return;
    }
    stats.probes += probes.len() as u32;
    let known_zero = known_zero_on_entry(func, &probes);
    let mut folded_values: Vec<ValueId> = Vec::new();
    for (&b, probe) in &probes {
        let insts = &func.blocks[b].insts[..probe.load];
        if known_zero[b] && !insts.iter().any(|i| is_clobber(&i.kind)) {
            func.blocks[b].term = Terminator::Br(probe.else_blk);
            folded_values.extend(probe.values);
            stats.folded += 1;
        }
    }
    if folded_values.is_empty() {
        return;
    }
    remove_unused(func, &folded_values);
    stats.blocks_removed += sweep_unreachable_blocks(func);
}

/// Knife 2 — a loop that cannot call leaves the flag where it found
/// it, yet its first probe reads it every iteration
/// (the entry state is unknown). Give such a loop one probe on the way
/// in: a fresh block every entry edge now goes through, branching to
/// the in-loop probe's own propagate path when the flag is set and
/// into the header otherwise. The in-loop probe is then reached known
/// zero on every path and the fold above removes it.
///
/// Taken only when the loop holds exactly one probe, its propagate
/// path lies wholly outside the loop, that path reads no value the
/// loop defines, and every store in the loop sits in a block the probe
/// block strictly dominates — so the first iteration writes nothing
/// before its probe, and the propagate path run before that iteration
/// sees exactly the memory it saw from the first probe (the copy loop
/// `dst.push(src[i])` probes after the read, stores after that). A
/// loop that runs zero times now probes once; that can differ only if
/// the flag was already set on entry, which no caller allows.
fn guard_loop_entries(func: &mut Function) -> u32 {
    let mut guarded: HashSet<BlockId> = HashSet::new();
    loop {
        let dom = DominatorTree::compute(func);
        let loops = LoopAnalysis::compute(func, &dom);
        let next = loops.loops().iter().find_map(|lp| {
            if guarded.contains(&lp.header) || lp.header.0 == 0 {
                return None;
            }
            guardable_probe(func, lp, &dom).map(|then_blk| (lp.clone(), then_blk))
        });
        let Some((lp, then_blk)) = next else {
            return guarded.len() as u32;
        };
        insert_entry_probe(func, &lp, then_blk);
        guarded.insert(lp.header);
    }
}

/// The propagate block of `lp`'s only probe, when the loop qualifies.
fn guardable_probe(func: &Function, lp: &NaturalLoop, dom: &DominatorTree) -> Option<BlockId> {
    let body: HashSet<usize> = lp.body.iter().map(|b| b.0 as usize).collect();
    let mut probes = body
        .iter()
        .filter_map(|&b| probe_of(func, b).map(|p| (b, p)));
    let (probe_blk, probe) = probes.next()?;
    if probes.next().is_some() {
        return None;
    }
    let probe_id = func.blocks[probe_blk].id;
    let store = |k: &InstKind| {
        matches!(
            k,
            InstKind::Store(..) | InstKind::StoreDyn(..) | InstKind::StoreDynScaled8(..)
        )
    };
    for &b in &body {
        let insts = &func.blocks[b].insts;
        if insts.iter().any(|i| is_clobber(&i.kind)) {
            return None;
        }
        let id = func.blocks[b].id;
        if insts.iter().any(|i| store(&i.kind)) && (id == probe_id || !dom.dominates(probe_id, id))
        {
            return None;
        }
    }
    let defined: HashSet<ValueId> = body
        .iter()
        .flat_map(|&b| func.blocks[b].insts.iter().filter_map(|i| i.result))
        .collect();
    let mut seen: HashSet<usize> = HashSet::new();
    let mut stack = vec![probe.then_blk.0 as usize];
    while let Some(b) = stack.pop() {
        if !seen.insert(b) {
            continue;
        }
        if body.contains(&b) {
            return None;
        }
        let block = &func.blocks[b];
        let mut reads_loop_value = false;
        for i in &block.insts {
            visit_value_operands(&i.kind, |v| reads_loop_value |= defined.contains(&v));
        }
        if let Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        }
        | Terminator::Ret(Some(Operand::Value(v))) = &block.term
        {
            reads_loop_value |= defined.contains(v);
        }
        if reads_loop_value {
            return None;
        }
        stack.extend(successors(&block.term).map(|s| s.0 as usize));
    }
    Some(probe.then_blk)
}

/// A new block probing the flag on every edge into `lp`'s header.
fn insert_entry_probe(func: &mut Function, lp: &NaturalLoop, then_blk: BlockId) {
    let header = lp.header;
    let entry = func.add_block();
    let p = func.append_inst(
        entry,
        InstKind::GlobalRef(THROW_ACTIVE_SYM.into()),
        Type::Ptr,
        None,
    );
    let v = func.append_inst(
        entry,
        InstKind::Load(Type::I64, Operand::Value(p), 0),
        Type::I64,
        None,
    );
    let c = func.append_inst(
        entry,
        InstKind::ICmp(IPred::Ne, Operand::Value(v), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    func.set_term(
        entry,
        Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk,
            else_blk: header,
        },
    );
    for block in func.blocks.iter_mut() {
        if block.id == entry || lp.body.contains(&block.id) {
            continue;
        }
        match &mut block.term {
            Terminator::Br(t) if *t == header => *t = entry,
            Terminator::CondBr {
                then_blk: t,
                else_blk: e,
                ..
            } => {
                for x in [t, e] {
                    if *x == header {
                        *x = entry;
                    }
                }
            }
            _ => {}
        }
    }
}

fn successors(term: &Terminator) -> impl Iterator<Item = BlockId> + '_ {
    let (a, b) = match term {
        Terminator::Br(t) => (Some(*t), None),
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => (Some(*then_blk), Some(*else_blk)),
        Terminator::Ret(_) | Terminator::Unreachable => (None, None),
    };
    a.into_iter().chain(b)
}

/// The probe that ends block `b`, if it ends in one.
fn probe_of(func: &Function, b: usize) -> Option<Probe> {
    let block = &func.blocks[b];
    let Terminator::CondBr {
        cond: Operand::Value(c),
        then_blk,
        else_blk,
    } = block.term
    else {
        return None;
    };
    if then_blk == else_blk {
        return None;
    }
    let def = |v: ValueId| block.insts.iter().position(|i| i.result == Some(v));
    let ci = def(c)?;
    let InstKind::ICmp(IPred::Ne, Operand::Value(v), Operand::ConstI64(0)) = block.insts[ci].kind
    else {
        return None;
    };
    let li = def(v)?;
    let InstKind::Load(Type::I64, Operand::Value(p), 0) = block.insts[li].kind else {
        return None;
    };
    let pi = def(p)?;
    if !matches!(&block.insts[pi].kind, InstKind::GlobalRef(sym) if sym == THROW_ACTIVE_SYM) {
        return None;
    }
    if block.insts[li + 1..].iter().any(|i| is_clobber(&i.kind)) {
        return None;
    }
    Some(Probe {
        load: li,
        values: [c, v, p],
        then_blk,
        else_blk,
    })
}

/// The only instructions that can change the flag.
fn is_clobber(kind: &InstKind) -> bool {
    matches!(kind, InstKind::Call(..) | InstKind::CallIndirect(..))
}

/// Per block: is the flag known to be zero when control enters it?
/// Optimistic start (every block but the entry known zero), lowered to
/// the greatest fixpoint.
fn known_zero_on_entry(func: &Function, probes: &HashMap<usize, Probe>) -> Vec<bool> {
    let n = func.blocks.len();
    let mut known = vec![true; n];
    known[0] = false;
    loop {
        let mut changed = false;
        for b in 0..n {
            let at_end = known[b] && !func.blocks[b].insts.iter().any(|i| is_clobber(&i.kind));
            for (succ, zero) in out_edges(&func.blocks[b].term, at_end, probes.get(&b)) {
                let s = succ.0 as usize;
                if known[s] && !zero {
                    known[s] = false;
                    changed = true;
                }
            }
        }
        if !changed {
            return known;
        }
    }
}

/// Each successor with whether the flag is known zero along that edge.
fn out_edges(term: &Terminator, at_end: bool, probe: Option<&Probe>) -> Vec<(BlockId, bool)> {
    match (term, probe) {
        (Terminator::CondBr { .. }, Some(p)) => vec![(p.then_blk, false), (p.else_blk, true)],
        (
            Terminator::CondBr {
                then_blk, else_blk, ..
            },
            None,
        ) => vec![(*then_blk, at_end), (*else_blk, at_end)],
        (Terminator::Br(t), _) => vec![(*t, at_end)],
        (Terminator::Ret(_) | Terminator::Unreachable, _) => Vec::new(),
    }
}

/// Drop the folded probes' compare / load / global_ref where nothing
/// else reads them (a load can still feed another use: keep it then).
fn remove_unused(func: &mut Function, candidates: &[ValueId]) {
    let mut left: HashSet<ValueId> = candidates.iter().copied().collect();
    loop {
        let mut used: HashSet<ValueId> = HashSet::new();
        for block in &func.blocks {
            for inst in &block.insts {
                visit_value_operands(&inst.kind, |v| {
                    used.insert(v);
                });
            }
            match &block.term {
                Terminator::CondBr {
                    cond: Operand::Value(v),
                    ..
                }
                | Terminator::Ret(Some(Operand::Value(v))) => {
                    used.insert(*v);
                }
                _ => {}
            }
        }
        let dead: HashSet<ValueId> = left.iter().copied().filter(|v| !used.contains(v)).collect();
        if dead.is_empty() {
            return;
        }
        for block in func.blocks.iter_mut() {
            block
                .insts
                .retain(|i| !i.result.is_some_and(|r| dead.contains(&r)));
        }
        left.retain(|v| !dead.contains(v));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::FuncId;

    /// Append a flag probe to `b` branching to `(then_blk, else_blk)`.
    fn probe(f: &mut Function, b: BlockId, then_blk: BlockId, else_blk: BlockId) {
        let p = f.append_inst(
            b,
            InstKind::GlobalRef(THROW_ACTIVE_SYM.into()),
            Type::Ptr,
            None,
        );
        let v = f.append_inst(
            b,
            InstKind::Load(Type::I64, Operand::Value(p), 0),
            Type::I64,
            None,
        );
        let c = f.append_inst(
            b,
            InstKind::ICmp(IPred::Ne, Operand::Value(v), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        f.set_term(
            b,
            Terminator::CondBr {
                cond: Operand::Value(c),
                then_blk,
                else_blk,
            },
        );
    }

    fn call(f: &mut Function, b: BlockId) {
        f.append_void(b, InstKind::Call(FuncId(0), Vec::new()));
    }

    fn run(f: &mut Function) -> ThrowCheckFoldStats {
        let mut stats = ThrowCheckFoldStats::default();
        fold_in_function(f, &mut stats);
        stats
    }

    /// entry: call; probe → (throw0, mid) · mid: [call?] probe → (throw1, exit)
    fn two_probes(call_between: bool) -> Function {
        let mut f = Function::new("t", Type::I64);
        let entry = f.add_block();
        let throw0 = f.add_block();
        let mid = f.add_block();
        let throw1 = f.add_block();
        let exit = f.add_block();
        call(&mut f, entry);
        probe(&mut f, entry, throw0, mid);
        f.set_term(throw0, Terminator::Ret(Some(Operand::ConstI64(0))));
        if call_between {
            call(&mut f, mid);
        }
        probe(&mut f, mid, throw1, exit);
        f.set_term(throw1, Terminator::Ret(Some(Operand::ConstI64(0))));
        f.set_term(exit, Terminator::Ret(Some(Operand::ConstI64(1))));
        f
    }

    #[test]
    fn a_probe_after_a_passed_probe_with_no_call_between_folds() {
        let mut f = two_probes(false);
        let stats = run(&mut f);
        assert_eq!(
            (stats.probes, stats.folded, stats.blocks_removed),
            (2, 1, 1)
        );
        assert_eq!(f.blocks.len(), 4);
    }

    #[test]
    fn a_call_between_the_probes_keeps_both() {
        let mut f = two_probes(true);
        let stats = run(&mut f);
        assert_eq!((stats.probes, stats.folded), (2, 0));
        assert_eq!(f.blocks.len(), 5);
    }

    #[test]
    fn the_first_probe_of_a_function_is_never_folded() {
        let mut f = Function::new("t", Type::I64);
        let entry = f.add_block();
        let throw0 = f.add_block();
        let exit = f.add_block();
        probe(&mut f, entry, throw0, exit);
        f.set_term(throw0, Terminator::Ret(Some(Operand::ConstI64(0))));
        f.set_term(exit, Terminator::Ret(Some(Operand::ConstI64(1))));
        assert_eq!(run(&mut f).folded, 0);
    }

    /// entry: call → head · head → (body, exit) · body: [store?] probe → (throw, head)
    fn read_only_loop(store_in_body: bool) -> Function {
        let mut f = Function::new("t", Type::I64);
        let entry = f.add_block();
        let head = f.add_block();
        let body = f.add_block();
        let throw = f.add_block();
        let exit = f.add_block();
        call(&mut f, entry);
        f.set_term(entry, Terminator::Br(head));
        f.set_term(
            head,
            Terminator::CondBr {
                cond: Operand::ConstBool(true),
                then_blk: body,
                else_blk: exit,
            },
        );
        if store_in_body {
            let slot = f.append_inst(entry, InstKind::Alloca(Type::I64), Type::Ptr, None);
            f.append_void(
                body,
                InstKind::Store(Operand::ConstI64(1), Operand::Value(slot), 0),
            );
        }
        probe(&mut f, body, throw, head);
        f.set_term(throw, Terminator::Ret(Some(Operand::ConstI64(0))));
        f.set_term(exit, Terminator::Ret(Some(Operand::ConstI64(1))));
        f
    }

    #[test]
    fn a_read_only_loop_probes_once_on_the_way_in() {
        let mut f = read_only_loop(false);
        let stats = run(&mut f);
        assert_eq!((stats.loop_entry_probes, stats.folded), (1, 1));
    }

    #[test]
    fn a_loop_that_stores_keeps_its_probe() {
        let mut f = read_only_loop(true);
        let stats = run(&mut f);
        assert_eq!((stats.loop_entry_probes, stats.folded), (0, 0));
    }

    #[test]
    fn a_loop_that_stores_only_after_its_probe_probes_once_on_the_way_in() {
        // entry: call → head · head → (body, exit)
        // body: probe → (throw, tail) · tail: store → head
        let mut f = Function::new("t", Type::I64);
        let entry = f.add_block();
        let head = f.add_block();
        let body = f.add_block();
        let throw = f.add_block();
        let tail = f.add_block();
        let exit = f.add_block();
        call(&mut f, entry);
        let slot = f.append_inst(entry, InstKind::Alloca(Type::I64), Type::Ptr, None);
        f.set_term(entry, Terminator::Br(head));
        f.set_term(
            head,
            Terminator::CondBr {
                cond: Operand::ConstBool(true),
                then_blk: body,
                else_blk: exit,
            },
        );
        probe(&mut f, body, throw, tail);
        f.append_void(
            tail,
            InstKind::Store(Operand::ConstI64(1), Operand::Value(slot), 0),
        );
        f.set_term(tail, Terminator::Br(head));
        f.set_term(throw, Terminator::Ret(Some(Operand::ConstI64(0))));
        f.set_term(exit, Terminator::Ret(Some(Operand::ConstI64(1))));
        let stats = run(&mut f);
        assert_eq!((stats.loop_entry_probes, stats.folded), (1, 1));
    }

    #[test]
    fn a_loop_probe_folds_only_when_every_way_in_has_passed_one() {
        // entry: probe → (throw, pre) · pre → head · head → (body, exit)
        // body: [call?] probe → (throw2, head)
        for (call_in_body, want) in [(false, 1), (true, 0)] {
            let mut f = Function::new("t", Type::I64);
            let entry = f.add_block();
            let throw0 = f.add_block();
            let head = f.add_block();
            let body = f.add_block();
            let throw2 = f.add_block();
            let exit = f.add_block();
            probe(&mut f, entry, throw0, head);
            f.set_term(throw0, Terminator::Ret(Some(Operand::ConstI64(0))));
            f.set_term(
                head,
                Terminator::CondBr {
                    cond: Operand::ConstBool(true),
                    then_blk: body,
                    else_blk: exit,
                },
            );
            if call_in_body {
                call(&mut f, body);
            }
            probe(&mut f, body, throw2, head);
            f.set_term(throw2, Terminator::Ret(Some(Operand::ConstI64(0))));
            f.set_term(exit, Terminator::Ret(Some(Operand::ConstI64(1))));
            assert_eq!(run(&mut f).folded, want, "call_in_body={call_in_body}");
        }
    }
}
