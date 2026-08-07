//! Versioning installation (phase 1b-ii) — the AOT analogue of
//! LLVM's LoopVersioning. The planned region is cloned before
//! mutation (the clone keeps the f64 shape, the originals become the
//! fast path); after mutation each growth site gets a window-check
//! chain that side-exits into the preserved loop, and an entry guard
//! re-routes the region's external entry edge.
//!
//! A side exit materializes the slow continuation's live state:
//! every cloned value live at the matching slow split point is
//! written back (`sitofp` for demoted values — exact, since every
//! fast-path in-set value is ≤ 2^53-1 in magnitude by the guard
//! invariant — and a plain `copy` for intermediates). Shared
//! (non-set) cells need no writeback: both versions define the same
//! id. The entry guard branches straight into the slow clone with no
//! writeback — no region state exists yet at the entry edge.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
};

use crate::rc_peephole::visit_value_operands;
use crate::slot_forward::rewrite_operands;

use super::cfg::{
    block_touches, multi_def_values, remap_term_operand, retarget, retarget_edge, successors,
    term_operand_value,
};
use super::guard::{GuardCheck, GuardSite};
use super::plan::RegionPlan;
use super::push_value;

/// The f64 clone of a planned region, taken before mutation.
pub(crate) struct RegionClone {
    pub(super) block_map: HashMap<BlockId, BlockId>,
    pub(super) value_map: HashMap<ValueId, ValueId>,
}

/// Clone the region blocks (appended, preserving the
/// `BlockId.0 == position` invariant). In-set values land on fresh
/// f64 ids, single-def intermediates on fresh same-type ids,
/// multi-def non-set cells stay shared.
pub(crate) fn clone_region(
    func: &mut Function,
    plan: &RegionPlan,
    set: &HashSet<ValueId>,
) -> RegionClone {
    let cells = multi_def_values(func);
    let mut value_map: HashMap<ValueId, ValueId> = HashMap::new();
    for b in &plan.region {
        for inst in &func.blocks[b.0 as usize].insts {
            let Some(r) = inst.result else { continue };
            if value_map.contains_key(&r) {
                continue;
            }
            if set.contains(&r) {
                value_map.insert(r, push_value(&mut func.values, Type::F64));
            } else if !cells.contains(&r) {
                let ty = func.values[r.0 as usize].ty;
                value_map.insert(r, push_value(&mut func.values, ty));
            }
        }
    }

    let mut block_map: HashMap<BlockId, BlockId> = HashMap::new();
    let base = func.blocks.len() as u32;
    for (i, b) in plan.region.iter().enumerate() {
        block_map.insert(*b, BlockId(base + i as u32));
    }

    let op_map: HashMap<ValueId, Operand> = value_map
        .iter()
        .map(|(k, v)| (*k, Operand::Value(*v)))
        .collect();
    for b in &plan.region {
        let src = func.blocks[b.0 as usize].clone();
        let insts = src
            .insts
            .into_iter()
            .map(|mut inst| {
                if let Some(r) = inst.result {
                    if let Some(nr) = value_map.get(&r) {
                        inst.result = Some(*nr);
                    }
                }
                rewrite_operands(&mut inst.kind, &op_map);
                inst
            })
            .collect();
        let term = retarget(remap_term_operand(src.term, &op_map), &block_map);
        func.blocks.push(Block {
            id: block_map[b],
            insts,
            term,
        });
    }
    RegionClone {
        block_map,
        value_map,
    }
}

/// Install guards after mutation. Order matters: the entry guard
/// re-routes external edges first (while the only appended blocks are
/// the clones — fast-path back edges created by later splits must not
/// be re-routed), then the slow blocks split at each site, then the
/// fast blocks get check chains and side exits.
pub(crate) fn install_guards(
    func: &mut Function,
    plan: &RegionPlan,
    clone: &RegionClone,
    set: &HashSet<ValueId>,
    fast_pos: &HashMap<(BlockId, usize), usize>,
) -> (u32, u32) {
    let mut entry_guards = 0u32;
    if !plan.entry_guard.is_empty() {
        let fast_entry = plan.entry;
        let slow_entry = clone.block_map[&fast_entry];
        let pre_len = func.blocks.len();
        let chain = build_check_chain(func, &plan.entry_guard, slow_entry, fast_entry);
        entry_guards = plan.entry_guard.len() as u32;
        let region: HashSet<BlockId> = plan.region.iter().copied().collect();
        for bi in 0..pre_len {
            if region.contains(&BlockId(bi as u32)) {
                continue;
            }
            retarget_edge(&mut func.blocks[bi].term, fast_entry, chain);
        }
    }

    // slow-side splits: one continuation block per site. Grouped into
    // a Vec in `plan.growth` order — deliberately not a HashMap. Both
    // this loop and the fast-side one below mint blocks and values, so
    // the grouping's iteration order picks their ids; hash order made
    // `tr build` emit two byte-different binaries for the same input
    // (the same reason `build_side_exit` sorts its live set). The
    // ordering is free semantically — the second loop reaches every
    // site through `slow_target` — so first-appearance keeps the ids
    // reading in source order. `plan.growth` is itself deterministic:
    // `collect_guard_sites` walks `func.blocks` in position order.
    let mut by_block: Vec<(BlockId, Vec<&GuardSite>)> = Vec::new();
    for s in &plan.growth {
        match by_block.iter_mut().find(|(b, _)| *b == s.block) {
            Some((_, sites)) => sites.push(s),
            None => by_block.push((s.block, vec![s])),
        }
    }
    let mut slow_target: HashMap<(BlockId, usize), BlockId> = HashMap::new();
    for (b, sites) in &mut by_block {
        sites.sort_by_key(|s| s.inst_idx);
        let idxs: Vec<usize> = sites.iter().map(|s| s.inst_idx).collect();
        let targets = split_block_at(func, clone.block_map[b], &idxs);
        for (s, t) in sites.iter().zip(targets) {
            slow_target.insert((s.block, s.inst_idx), t);
        }
    }

    let live_in = slow_liveness(func, clone);
    let inverse: HashMap<ValueId, ValueId> =
        clone.value_map.iter().map(|(k, v)| (*v, *k)).collect();

    // fast-side splits + check chains + side exits. A post site
    // splits after its (already-run) op — the checks see the result;
    // its side exit still targets the slow split before the op, so
    // the slow path recomputes it and only operands write back. The
    // first check of every site inlines into the preceding segment
    // (cmp appended, terminator becomes the branch) — the common
    // single-check site then adds no extra hot-path jump.
    let mut guards = 0u32;
    for (b, sites) in by_block {
        let idxs: Vec<usize> = sites
            .iter()
            .map(|s| fast_pos[&(s.block, s.inst_idx)] + s.post as usize)
            .collect();
        debug_assert!(idxs.is_sorted(), "mutation preserves inst order");
        let conts = split_block_at(func, b, &idxs);
        let mut prev = b;
        for (s, cont) in sites.iter().zip(conts) {
            let target = slow_target[&(s.block, s.inst_idx)];
            let exit = build_side_exit(func, target, &live_in, &inverse, set);
            let (first, rest) = s.checks.split_first().expect("guarded site has checks");
            let next = if rest.is_empty() {
                cont
            } else {
                build_check_chain(func, rest, exit, cont)
            };
            let cond = push_value(&mut func.values, Type::Bool);
            let head = &mut func.blocks[prev.0 as usize];
            head.insts.push(Inst {
                result: Some(cond),
                kind: InstKind::ICmp(first.pred, first.operand, Operand::ConstI64(first.bound)),
                origin: None,
            });
            head.term = Terminator::CondBr {
                cond: Operand::Value(cond),
                then_blk: exit,
                else_blk: next,
            };
            prev = cont;
            guards += s.checks.len() as u32;
        }
    }
    (guards, entry_guards)
}

/// Split `block` before each inst index (ascending); continuation
/// blocks append (chained head → c0 → c1 → … → original terminator).
/// Returns the continuation starting at each index.
pub(super) fn split_block_at(func: &mut Function, block: BlockId, idxs: &[usize]) -> Vec<BlockId> {
    let src = &mut func.blocks[block.0 as usize];
    let term = std::mem::replace(&mut src.term, Terminator::Unreachable);
    let insts = std::mem::take(&mut src.insts);

    let mut segments: Vec<Vec<Inst>> = Vec::with_capacity(idxs.len() + 1);
    let mut cur: Vec<Inst> = Vec::new();
    let mut next_split = 0usize;
    for (i, inst) in insts.into_iter().enumerate() {
        if next_split < idxs.len() && i == idxs[next_split] {
            segments.push(std::mem::take(&mut cur));
            next_split += 1;
        }
        cur.push(inst);
    }
    segments.push(cur);

    let first = segments.remove(0);
    let mut targets: Vec<BlockId> = Vec::with_capacity(segments.len());
    let base = func.blocks.len() as u32;
    for (i, seg) in segments.into_iter().enumerate() {
        let id = BlockId(base + i as u32);
        targets.push(id);
        func.blocks.push(Block {
            id,
            insts: seg,
            term: Terminator::Unreachable,
        });
    }
    let mut prev = block;
    for t in &targets {
        func.blocks[prev.0 as usize].term = Terminator::Br(*t);
        prev = *t;
    }
    func.blocks[prev.0 as usize].term = term;
    func.blocks[block.0 as usize].insts = first;
    targets
}

/// Backward liveness of cloned (fresh) values over the slow blocks
/// (including split continuations). Cloned values cannot escape the
/// region (feasibility), so live-out at region exits is empty.
fn slow_liveness(func: &Function, clone: &RegionClone) -> HashMap<BlockId, HashSet<ValueId>> {
    let fresh: HashSet<ValueId> = clone.value_map.values().copied().collect();
    let mut slow_blocks: Vec<BlockId> = clone.block_map.values().copied().collect();
    let mut seen: HashSet<BlockId> = slow_blocks.iter().copied().collect();
    let mut stack = slow_blocks.clone();
    while let Some(b) = stack.pop() {
        for s in successors(&func.blocks[b.0 as usize].term) {
            if !seen.contains(&s) && block_touches(func, s, &fresh) {
                seen.insert(s);
                slow_blocks.push(s);
                stack.push(s);
            }
        }
    }

    let mut live_in: HashMap<BlockId, HashSet<ValueId>> = HashMap::new();
    let mut changed = true;
    while changed {
        changed = false;
        for &b in slow_blocks.iter().rev() {
            let block = &func.blocks[b.0 as usize];
            let mut live: HashSet<ValueId> = HashSet::new();
            for s in successors(&block.term) {
                if let Some(li) = live_in.get(&s) {
                    live.extend(li.iter().copied());
                }
            }
            if let Some(v) = term_operand_value(&block.term) {
                if fresh.contains(&v) {
                    live.insert(v);
                }
            }
            for inst in block.insts.iter().rev() {
                if let Some(r) = inst.result {
                    live.remove(&r);
                }
                visit_value_operands(&inst.kind, |v| {
                    if fresh.contains(&v) {
                        live.insert(v);
                    }
                });
            }
            let entry = live_in.entry(b).or_default();
            if *entry != live {
                *entry = live;
                changed = true;
            }
        }
    }
    live_in
}

/// A side-exit block: write back every cloned value live at the slow
/// continuation, then branch into it. Demoted values bridge through
/// `sitofp` (exact by the guard invariant), intermediates copy.
fn build_side_exit(
    func: &mut Function,
    target: BlockId,
    live_in: &HashMap<BlockId, HashSet<ValueId>>,
    inverse: &HashMap<ValueId, ValueId>,
    set: &HashSet<ValueId>,
) -> BlockId {
    let mut live: Vec<ValueId> = live_in
        .get(&target)
        .map(|s| s.iter().copied().collect())
        .unwrap_or_default();
    live.sort_by_key(|v| v.0);
    let mut insts: Vec<Inst> = Vec::with_capacity(live.len());
    for f in live {
        let o = inverse[&f];
        let kind = if set.contains(&o) {
            InstKind::SiToFp(Operand::Value(o))
        } else {
            InstKind::Copy(func.values[o.0 as usize].ty, Operand::Value(o))
        };
        insts.push(Inst {
            result: Some(f),
            kind,
            origin: None,
        });
    }
    let id = BlockId(func.blocks.len() as u32);
    func.blocks.push(Block {
        id,
        insts,
        term: Terminator::Br(target),
    });
    id
}

/// A chain of check blocks: each violation branches to `on_fail`,
/// passing all checks reaches `on_pass`. Returns the chain head.
fn build_check_chain(
    func: &mut Function,
    checks: &[GuardCheck],
    on_fail: BlockId,
    on_pass: BlockId,
) -> BlockId {
    debug_assert!(!checks.is_empty());
    let head = BlockId(func.blocks.len() as u32);
    for (i, c) in checks.iter().enumerate() {
        let cond = push_value(&mut func.values, Type::Bool);
        let id = BlockId(func.blocks.len() as u32);
        let next = if i + 1 == checks.len() {
            on_pass
        } else {
            BlockId(id.0 + 1)
        };
        func.blocks.push(Block {
            id,
            insts: vec![Inst {
                result: Some(cond),
                kind: InstKind::ICmp(c.pred, c.operand, Operand::ConstI64(c.bound)),
                origin: None,
            }],
            term: Terminator::CondBr {
                cond: Operand::Value(cond),
                then_blk: on_fail,
                else_blk: next,
            },
        });
    }
    head
}
