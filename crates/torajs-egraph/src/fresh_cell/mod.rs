//! Store→load forwarding and dead-store elimination over a freshly
//! allocated heap cell — LLVM's store-to-load forwarding and DSE
//! restricted to the path from a `noalias` allocation, applied to
//! `obj_alloc`. The factory a `new C()` inlines writes the header,
//! the class tag and every field default into the cell; the ctor body
//! then reads the header word back for a frozen-object guard before
//! each field write, and stores the real value over the default.
//! Nothing else can reach the cell until its pointer escapes, so along
//! the straight path from the allocation the last store to an offset
//! is what a load there reads, and a store overwritten before anything
//! read it never had an effect.
//!
//! The path is the allocation's block and the chain of successors each
//! of which has the chain as its only predecessor. It grows through a
//! conditional branch whose compare the forwarded values decide — the
//! guard's `load +4; and; icmp ne 0` over the header word the factory
//! just wrote — by taking that arm; the other arm and whatever only it
//! reached leave the cfg. The path ends at the first use of the pointer
//! outside address position (a call argument, a stored value, a copy,
//! a box, a retain): from there the runtime or another pointer may read
//! or write the cell, so a guard after a call that took the pointer is
//! kept — `Object.freeze(this)` in a ctor still throws on the next
//! write. Along the path, a load through the pointer that does not
//! forward keeps every pending store (it read the bytes), a
//! dynamic-offset store drops every pending entry, and an overlapping
//! access of a different width drops what it overlaps.
//!
//! A frame cell (`alloca_bytes`, heap_to_stack's promoted cells among
//! them) whose address never leaves address position anywhere in the
//! function is fresh in the same sense each time its instruction runs:
//! no pointer from an earlier run can reach it.
//!
//! After the slot passes (the inlined ctor's `this` reaches the cell
//! through a param spill they forward), before heap_to_stack (fewer
//! uses left to judge). `TORAJS_FRESH_CELL_OFF=1` skips (bisect gate).

pub(crate) mod fold;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    BlockId, FuncId, Function, InstKind, Module, Operand, Terminator, ValueId, visit_value_operands,
};

use crate::cell_access::{fits, load_width, store_width};
use crate::rc_peephole::collect_unescaped_slots;
use crate::slot_forward::{resolve, rewrite_operands};

const OBJ_ALLOC: &str = "__torajs_obj_alloc";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FreshCellStats {
    /// Loads replaced by the value the last store on the path wrote.
    pub loads_forwarded: u32,
    /// Stores deleted because a later store overwrote them unread.
    pub stores_removed: u32,
    /// Conditional branches decided by forwarded values.
    pub branches_folded: u32,
}

pub fn forward_fresh_cells(module: &mut Module) -> FreshCellStats {
    let mut stats = FreshCellStats::default();
    let alloc = module
        .funcs
        .iter()
        .position(|f| f.name == OBJ_ALLOC)
        .map(|i| FuncId(i as u32));
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        forward_in_function(func, alloc, &mut stats);
    }
    stats
}

/// A store on the path whose bytes a later load may read.
struct Pending {
    val: Operand,
    width: u64,
    at: (usize, usize),
    /// A load that could not forward has read the bytes: the store
    /// stays even once overwritten.
    observed: bool,
}

/// What the walks decided, applied to the function afterwards.
struct Walker {
    preds: Vec<Vec<BlockId>>,
    defs: HashMap<ValueId, (usize, usize)>,
    replace: HashMap<ValueId, Operand>,
    /// Positions of forwarded loads and dead stores.
    dead: HashSet<(usize, usize)>,
    /// Results whose defs a decided branch's compare chain evaluated.
    chain: Vec<ValueId>,
    stats: FreshCellStats,
}

pub(crate) fn forward_in_function(
    func: &mut Function,
    alloc: Option<FuncId>,
    stats: &mut FreshCellStats,
) {
    let mut w = Walker {
        preds: fold::reachable_preds(func),
        defs: def_sites(func),
        replace: HashMap::new(),
        dead: HashSet::new(),
        chain: Vec::new(),
        stats: FreshCellStats::default(),
    };
    // a frame cell is fresh only while nothing can hold its address
    // from an earlier run of the same instruction
    let frame = collect_unescaped_slots(func);
    for bi in 0..func.blocks.len() {
        for ii in 0..func.blocks[bi].insts.len() {
            let inst = &func.blocks[bi].insts[ii];
            let fresh = match &inst.kind {
                InstKind::Call(f, _) => Some(*f) == alloc,
                InstKind::AllocaBytes(_) => inst.result.is_some_and(|r| frame.contains(&r)),
                _ => false,
            };
            if let (true, Some(r)) = (fresh, inst.result) {
                w.walk(func, r, (bi, ii + 1));
            }
        }
    }
    apply(func, w, stats);
}

fn def_sites(func: &Function) -> HashMap<ValueId, (usize, usize)> {
    let mut defs = HashMap::new();
    for (bi, block) in func.blocks.iter().enumerate() {
        for (ii, inst) in block.insts.iter().enumerate() {
            if let Some(r) = inst.result {
                defs.insert(r, (bi, ii));
            }
        }
    }
    defs
}

impl Walker {
    fn walk(&mut self, func: &mut Function, base: ValueId, start: (usize, usize)) {
        let mut live: HashMap<u64, Pending> = HashMap::new();
        let (mut bi, mut ii) = start;
        let mut visited: HashSet<usize> = HashSet::new();
        loop {
            visited.insert(bi);
            for i in ii..func.blocks[bi].insts.len() {
                if !self.step(func, base, (bi, i), &mut live) {
                    return;
                }
            }
            let Some(next) = self.next_block(func, bi) else {
                return;
            };
            if visited.contains(&next) || self.preds[next].as_slice() != [BlockId(bi as u32)] {
                return;
            }
            (bi, ii) = (next, 0);
        }
    }

    /// The block the path continues into: a plain branch's target, or
    /// the arm a conditional takes once forwarded values decide it.
    fn next_block(&mut self, func: &mut Function, bi: usize) -> Option<usize> {
        match func.blocks[bi].term.clone() {
            Terminator::Br(t) => Some(t.0 as usize),
            Terminator::CondBr {
                cond,
                then_blk,
                else_blk,
            } => {
                let decided =
                    fold::const_eval(func, &self.defs, &self.replace, &cond, &mut self.chain);
                let Some(Operand::ConstBool(k)) = decided else {
                    return None;
                };
                let t = if k { then_blk } else { else_blk };
                func.blocks[bi].term = Terminator::Br(t);
                self.preds = fold::reachable_preds(func);
                self.stats.branches_folded += 1;
                Some(t.0 as usize)
            }
            Terminator::Ret(_) | Terminator::Unreachable => None,
        }
    }

    /// One instruction on the path; `false` ends the window.
    fn step(
        &mut self,
        func: &Function,
        base: ValueId,
        at: (usize, usize),
        live: &mut HashMap<u64, Pending>,
    ) -> bool {
        let inst = &func.blocks[at.0].insts[at.1];
        let is_base = |op: &Operand| *op == Operand::Value(base);
        match &inst.kind {
            InstKind::Store(val, p, off) if is_base(p) && !is_base(val) => {
                let width = store_width(func, val);
                live.retain(|&o, e| {
                    let overlap = o < off + width && *off < o + e.width;
                    if overlap && o == *off && e.width == width && !e.observed {
                        self.dead.insert(e.at);
                        self.stats.stores_removed += 1;
                    }
                    !overlap
                });
                let val = resolve(val, &self.replace);
                live.insert(
                    *off,
                    Pending {
                        val,
                        width,
                        at,
                        observed: false,
                    },
                );
                true
            }
            InstKind::Load(ty, p, off) if is_base(p) => {
                let hit = live
                    .get(off)
                    .filter(|e| e.width == load_width(*ty) && fits(func, &e.val, *ty))
                    .map(|e| e.val);
                match (hit, inst.result) {
                    (Some(v), Some(r)) => {
                        self.replace.insert(r, v);
                        self.dead.insert(at);
                        self.stats.loads_forwarded += 1;
                    }
                    _ => live.values_mut().for_each(|e| e.observed = true),
                }
                true
            }
            InstKind::LoadDyn(_, p, i)
            | InstKind::LoadDynScaled8(_, p, i)
            | InstKind::LoadU8Dyn(p, i)
            | InstKind::LoadU16Dyn(p, i)
                if is_base(p) && !is_base(i) =>
            {
                live.values_mut().for_each(|e| e.observed = true);
                true
            }
            InstKind::StoreDyn(v, p, i) | InstKind::StoreDynScaled8(v, p, i)
                if is_base(p) && !is_base(v) && !is_base(i) =>
            {
                live.clear();
                true
            }
            // a pointer compare reads no memory and leaks no pointer
            InstKind::ICmp(..) => true,
            kind => {
                let mut escapes = false;
                visit_value_operands(kind, |v| escapes |= v == base);
                !escapes
            }
        }
    }
}

fn apply(func: &mut Function, w: Walker, stats: &mut FreshCellStats) {
    if !w.replace.is_empty() {
        for block in func.blocks.iter_mut() {
            for inst in block.insts.iter_mut() {
                rewrite_operands(&mut inst.kind, &w.replace);
            }
            match &mut block.term {
                Terminator::Ret(Some(op)) => *op = resolve(op, &w.replace),
                Terminator::CondBr { cond, .. } => *cond = resolve(cond, &w.replace),
                _ => {}
            }
        }
    }
    let mut by_block: HashMap<usize, HashSet<usize>> = HashMap::new();
    for (bi, ii) in w.dead {
        by_block.entry(bi).or_default().insert(ii);
    }
    for (bi, gone) in by_block {
        let mut i = 0;
        func.blocks[bi].insts.retain(|_| {
            let keep = !gone.contains(&i);
            i += 1;
            keep
        });
    }
    if w.stats.branches_folded > 0 {
        fold::sweep_dead_chain(func, &w.chain);
        fold::detach_unreachable(func);
    }
    stats.loads_forwarded += w.stats.loads_forwarded;
    stats.stores_removed += w.stats.stores_removed;
    stats.branches_folded += w.stats.branches_folded;
}
