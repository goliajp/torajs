//! Where the counter's defs go: the block its start is computed in,
//! and the steps of the loop counter it follows.

use crate::id_hash::HashSet;

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::{LoopAnalysis, NaturalLoop};
use torajs_core::ssa::{
    BinOp, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
};

use super::At;
use crate::loop_rotate::duplicable;

pub(super) struct Cfg<'a> {
    pub(super) func: &'a Function,
    /// the defs of value `v` are `def_at[def_start[v]..def_start[v + 1]]`
    def_start: Vec<u32>,
    def_at: Vec<At>,
    /// by block index (the ids are dense)
    preds: Vec<Vec<BlockId>>,
    /// by block index: inside some loop
    looped: Vec<bool>,
}

impl<'a> Cfg<'a> {
    pub(super) fn of(func: &'a Function, la: &LoopAnalysis) -> Self {
        let n = func.values.len();
        let mut def_start = vec![0u32; n + 1];
        for inst in func.blocks.iter().flat_map(|b| &b.insts) {
            if let Some(r) = inst.result {
                def_start[r.0 as usize + 1] += 1;
            }
        }
        for v in 0..n {
            def_start[v + 1] += def_start[v];
        }
        let mut fill = def_start.clone();
        let mut def_at = vec![(0, 0); def_start[n] as usize];
        let mut preds = vec![Vec::new(); func.blocks.len()];
        for (b, block) in func.blocks.iter().enumerate() {
            for (k, inst) in block.insts.iter().enumerate() {
                if let Some(r) = inst.result {
                    let slot = &mut fill[r.0 as usize];
                    def_at[*slot as usize] = (b, k);
                    *slot += 1;
                }
            }
            let succs: &[BlockId] = match &block.term {
                Terminator::Br(t) => &[*t],
                Terminator::CondBr {
                    then_blk, else_blk, ..
                } => &[*then_blk, *else_blk],
                _ => &[],
            };
            for s in succs {
                preds[s.0 as usize].push(block.id);
            }
        }
        let mut looped = vec![false; func.blocks.len()];
        for b in la.loops().iter().flat_map(|l| &l.body) {
            looped[b.0 as usize] = true;
        }
        Cfg {
            func,
            def_start,
            def_at,
            preds,
            looped,
        }
    }

    pub(super) fn defs(&self, v: ValueId) -> &[At] {
        let v = v.0 as usize;
        &self.def_at[self.def_start[v] as usize..self.def_start[v + 1] as usize]
    }

    pub(super) fn preds(&self, b: BlockId) -> &[BlockId] {
        &self.preds[b.0 as usize]
    }

    /// The instruction defining `v`, when it has exactly one.
    pub(super) fn one_def(&self, v: ValueId) -> Option<&'a InstKind> {
        let [(b, k)] = self.defs(v) else {
            return None;
        };
        Some(&self.func.blocks[*b].insts[*k].kind)
    }

    /// What `x` holds at `(b, at)` when it is the one-def copy of a
    /// value that nothing redefines on the way to `at` (a loop body's
    /// `%x = copy %cell` in the header, read further down); `x`
    /// otherwise. The way is the run of one-predecessor blocks above
    /// `b` back to the copy.
    pub(super) fn copy_source(&self, b: usize, at: usize, x: ValueId) -> ValueId {
        let [(xb, j)] = self.defs(x) else {
            return x;
        };
        let Some(InstKind::Copy(Type::I64, Operand::Value(src))) =
            self.func.blocks[*xb].insts.get(*j).map(|d| &d.kind)
        else {
            return x;
        };
        let sets = |insts: &[Inst]| insts.iter().any(|d| d.result == Some(*src));
        let (mut cur, mut upto) = (b, at);
        for _ in 0..8 {
            let insts = &self.func.blocks[cur].insts;
            if cur == *xb {
                return if *j < upto && !sets(&insts[j + 1..upto]) {
                    *src
                } else {
                    x
                };
            }
            if sets(&insts[..upto]) {
                return x;
            }
            let [p] = self.preds(BlockId(cur as u32)) else {
                return x;
            };
            cur = p.0 as usize;
            upto = self.func.blocks[cur].insts.len();
        }
        x
    }

    /// `v` is set at most once per call: a parameter nothing redefines,
    /// or one def outside every loop.
    pub(super) fn once(&self, v: ValueId) -> bool {
        match self.defs(v) {
            [] => self.func.params.contains(&v),
            [(b, _)] => !self.looped[*b],
            _ => false,
        }
    }

    pub(super) fn defined_in(&self, v: ValueId, lp: &NaturalLoop) -> bool {
        self.defs(v)
            .iter()
            .any(|d| lp.contains(&BlockId(d.0 as u32)))
    }

    /// `v` holds the same value at the end of `entry` and anywhere
    /// after: a parameter, or every def sits in a block dominating it.
    pub(super) fn settled_at(&self, v: ValueId, entry: BlockId, dom: &DominatorTree) -> bool {
        match self.defs(v) {
            [] => self.func.params.contains(&v),
            ds => ds.iter().all(|d| dom.dominates(BlockId(d.0 as u32), entry)),
        }
    }

    /// Every def of `i` inside `lp` is `i = copy t` with `t = add i, 1`
    /// earlier in the same block and no def of `i` between; answers
    /// those defs.
    pub(super) fn steps_of(&self, i: ValueId, lp: &NaturalLoop) -> Option<Vec<At>> {
        let mut steps = Vec::new();
        for &(b, k) in self
            .defs(i)
            .iter()
            .filter(|d| lp.contains(&BlockId(d.0 as u32)))
        {
            let insts = &self.func.blocks[b].insts;
            let InstKind::Copy(Type::I64, Operand::Value(t)) = insts[k].kind else {
                return None;
            };
            let [(tb, tk)] = self.defs(t) else {
                return None;
            };
            if *tb != b || *tk >= k {
                return None;
            }
            let plus_one = matches!(
                &insts[*tk].kind,
                InstKind::BinOp(BinOp::Add, Operand::Value(a), Operand::ConstI64(1))
                    | InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::Value(a)) if *a == i
            );
            if !plus_one || insts[tk + 1..k].iter().any(|x| x.result == Some(i)) {
                return None;
            }
            steps.push((b, k));
        }
        (!steps.is_empty()).then_some(steps)
    }

    /// The steps end the iterations: each back edge of `lp` leaves a
    /// block holding exactly one step, no other block steps, and no
    /// loop inside `lp` holds one — so every pass through the header
    /// follows exactly one step, and a counter moved at the top of the
    /// header keeps up with `i`.
    pub(super) fn steps_close_iterations(
        &self,
        la: &LoopAnalysis,
        lp: &NaturalLoop,
        steps: &[At],
    ) -> bool {
        let blocks: Vec<BlockId> = steps.iter().map(|s| BlockId(s.0 as u32)).collect();
        let mut sorted = blocks.clone();
        sorted.sort_by_key(|b| b.0);
        sorted.dedup();
        sorted.len() == blocks.len()
            && sorted == lp.back_edge_sources
            && !la.loops().iter().any(|l| {
                l.header != lp.header
                    && lp.contains(&l.header)
                    && blocks.iter().any(|b| l.contains(b))
            })
    }

    /// The block that runs once per trip around `lp`. A top-tested
    /// loop whose header holds only what loop_rotate copies into the
    /// latch (a bare exit test) takes the move in the body's first
    /// block, the header's one successor inside the loop, when nothing
    /// else enters it: the header stays a test the loop is rotated on.
    /// Any other loop takes it at the top of the header, which keeps it
    /// unrotated as it was (a header that held the dividend's copy
    /// would otherwise turn bare once the copy dies, and the rotated
    /// latch, ending in the test, is no longer copied into the arms).
    pub(super) fn trip_block(&self, lp: &NaturalLoop) -> BlockId {
        let h = lp.header;
        let block = &self.func.blocks[h.0 as usize];
        let bare = block.insts.iter().all(|i| duplicable(&i.kind));
        if let Terminator::CondBr {
            then_blk, else_blk, ..
        } = &block.term
            && bare
        {
            let inside = [*then_blk, *else_blk]
                .into_iter()
                .filter(|b| lp.contains(b));
            if let [b] = inside.collect::<Vec<_>>()[..]
                && b != h
                && self.preds(b) == [h]
            {
                return b;
            }
        }
        h
    }

    /// The block the counter starts in: the header's immediate
    /// dominator, when no block on a way from it into the loop (other
    /// than through the loop itself) redefines `i` — so what `i` holds
    /// at its end is what the loop starts from.
    pub(super) fn entry_block(
        &self,
        dom: &DominatorTree,
        lp: &NaturalLoop,
        i: ValueId,
    ) -> Option<BlockId> {
        let entry = dom.immediate_dominator(lp.header)?;
        let defines_i = |b: BlockId| {
            self.func.blocks[b.0 as usize]
                .insts
                .iter()
                .any(|x| x.result == Some(i))
        };
        let mut seen = HashSet::default();
        let mut stack: Vec<BlockId> = self
            .preds(lp.header)
            .iter()
            .copied()
            .filter(|p| !lp.contains(p))
            .collect();
        while let Some(b) = stack.pop() {
            if b == entry || !seen.insert(b) {
                continue;
            }
            if defines_i(b) {
                return None;
            }
            stack.extend(self.preds(b));
        }
        Some(entry)
    }

    /// `i` is 0 at the end of `entry`: the last def of `i` met walking
    /// back from there through blocks with one predecessor is `copy 0`.
    pub(super) fn zero_at(&self, entry: BlockId, i: ValueId) -> bool {
        let mut b = entry;
        for _ in 0..32 {
            let insts = &self.func.blocks[b.0 as usize].insts;
            if let Some(last) = insts.iter().rev().find(|x| x.result == Some(i)) {
                return matches!(last.kind, InstKind::Copy(_, Operand::ConstI64(0)));
            }
            match self.preds(b) {
                [p] => b = *p,
                _ => return false,
            }
        }
        false
    }
}
