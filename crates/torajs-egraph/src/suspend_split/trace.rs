//! What the frame's trace reports while the function is parked at a
//! suspension point: the frame words holding the values the body owns
//! there.
//!
//! The lowering lists those values on each suspension (its `held`
//! operands: owned locals, the temps a throw would release, the
//! operand a direct await keeps). Each is found in the frame as the
//! word of the cell it crossed in: its own slot when it was demoted,
//! the local's slot it was just read from, or a slot that lives in the
//! frame. A held value that crosses in no word is not the body's to
//! release once it lands, and is not reported. A value listed twice
//! is two references, reported twice.
//!
//! The trace (built by the lowering with the links and the parameter
//! words) then reads the state and, for each point that keeps any
//! word, visits those words with index `TRACE_STATE_WORD | word`.

use torajs_core::ssa::id_hash::HashMap;
use torajs_core::ssa::suspend::{FRAME_STATE_OFF, TRACE_STATE_WORD};
use torajs_core::ssa::{
    BinOp, BlockId, Function, InstKind, Operand, SigId, Terminator, Type, ValueId,
};

use super::cut::{Point, fresh_block};
use super::frame::Layout;

/// Per point, the frame offsets of the words it keeps.
pub(super) fn point_words(
    f: &Function,
    points: &[Point],
    slots: &HashMap<ValueId, ValueId>,
    layout: &Layout,
    cells: &[Vec<(ValueId, u64, Type)>],
) -> Vec<Vec<u64>> {
    let mut def: HashMap<ValueId, (usize, usize)> = HashMap::default();
    let mut stored: HashMap<ValueId, Vec<(ValueId, usize, usize)>> = HashMap::default();
    for (bi, b) in f.blocks.iter().enumerate() {
        for (ii, i) in b.insts.iter().enumerate() {
            if let Some(r) = i.result {
                def.insert(r, (bi, ii));
            }
            if let InstKind::Store(Operand::Value(v), Operand::Value(a), 0) = i.kind {
                stored.entry(v).or_default().push((a, bi, ii));
            }
        }
    }
    let (succ, pred) = edges(f);
    let held_at = |p: &Point| match f.blocks[p.cut].insts.last().map(|i| &i.kind) {
        Some(InstKind::Suspend(_, _, _, held)) => held.clone(),
        _ => unreachable!("a cut block ends at its suspension"),
    };
    (points.iter().zip(cells))
        .map(|(p, cells)| {
            let crossing: HashMap<ValueId, u64> =
                (cells.iter()).map(|&(cell, off, _)| (cell, off)).collect();
            let find = Find {
                f,
                def: &def,
                stored: &stored,
                slots,
                layout,
                crossing: &crossing,
                cut: p.cut,
                succ: &succ,
                to_cut: reach(p.cut, &pred),
            };
            (held_at(p).iter())
                .filter_map(|h| match h {
                    Operand::Value(v) => find.word(*v),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

struct Find<'a> {
    f: &'a Function,
    def: &'a HashMap<ValueId, (usize, usize)>,
    /// value → the whole-word stores of it: (cell, block, index)
    stored: &'a HashMap<ValueId, Vec<(ValueId, usize, usize)>>,
    slots: &'a HashMap<ValueId, ValueId>,
    layout: &'a Layout,
    /// the cells crossing this point: alloca → frame offset
    crossing: &'a HashMap<ValueId, u64>,
    cut: usize,
    succ: &'a [Vec<usize>],
    /// the blocks that reach the cut block
    to_cut: Vec<bool>,
}

impl Find<'_> {
    fn word(&self, h: ValueId) -> Option<u64> {
        if let Some(slot) = self.slots.get(&h) {
            return self.crossing.get(slot).copied();
        }
        self.read_from(h).or_else(|| self.stored_into(h))
    }

    /// `h` read from a cell that still holds it at the park.
    fn read_from(&self, h: ValueId) -> Option<u64> {
        let &(db, di) = self.def.get(&h)?;
        let InstKind::Load(_, Operand::Value(a), off) = self.f.blocks[db].insts[di].kind else {
            return None;
        };
        if let Some(&base) = self.layout.moved.get(&a) {
            return Some(base + off);
        }
        let word = *self.crossing.get(&a).filter(|_| off == 0)?;
        (!self.stored_between(a, db, di)).then_some(word)
    }

    /// `h` stored into a cell nothing stores to again before the park
    /// (a read of the cell the e-graph forwarded the store to).
    fn stored_into(&self, h: ValueId) -> Option<u64> {
        (self.stored.get(&h)?.iter())
            .filter(|&&(_, sb, _)| sb == self.cut || self.to_cut[sb])
            .find_map(|&(a, sb, si)| {
                let word = *self.crossing.get(&a)?;
                (!self.stored_between(a, sb, si)).then_some(word)
            })
    }

    /// Is there a store to `a` on a path from instruction `di` of
    /// block `db` to the end of the cut block?
    fn stored_between(&self, a: ValueId, db: usize, di: usize) -> bool {
        let stores = |b: usize, from: usize| {
            (self.f.blocks[b].insts[from..].iter())
                .any(|i| matches!(i.kind, InstKind::Store(_, Operand::Value(p), _) if p == a))
        };
        if stores(db, di + 1) {
            return true;
        }
        if db == self.cut {
            return false;
        }
        // blocks reached from `db` that reach the cut block
        let from_def = reach(db, self.succ);
        (0..from_def.len()).any(|b| {
            let between = from_def[b] && (self.to_cut[b] || b == self.cut);
            between && stores(b, 0)
        })
    }
}

/// Each block's successors and predecessors, by position.
fn edges(f: &Function) -> (Vec<Vec<usize>>, Vec<Vec<usize>>) {
    let n = f.blocks.len();
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut pred: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (b, blk) in f.blocks.iter().enumerate() {
        let to: &[_] = match &blk.term {
            Terminator::Br(t) => &[*t],
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => &[*then_blk, *else_blk],
            _ => &[],
        };
        for t in to {
            succ[b].push(t.0 as usize);
            pred[t.0 as usize].push(b);
        }
    }
    (succ, pred)
}

/// The blocks reached from `start` along `edges` (`start` itself only
/// through a cycle).
fn reach(start: usize, edges: &[Vec<usize>]) -> Vec<bool> {
    let mut seen = vec![false; edges.len()];
    let mut stack: Vec<usize> = edges[start].clone();
    while let Some(b) = stack.pop() {
        if !std::mem::replace(&mut seen[b], true) {
            stack.extend(&edges[b]);
        }
    }
    seen
}

/// Add the points' words to `trace`, whose entry block reports the
/// links and the parameter words and returns.
pub(super) fn add_points(trace: &mut Function, words: &[Vec<u64>], visit_sig: SigId) {
    if words.iter().all(Vec::is_empty) {
        return;
    }
    let [frame, visit, ctx] = trace.params[..] else {
        unreachable!("a frame trace takes (frame, visit, ctx)")
    };
    let entry = BlockId(0);
    let exit = fresh_block(trace);
    trace.blocks[exit.0 as usize].term = Terminator::Ret(None);
    let state = trace.append_inst(
        entry,
        InstKind::Load(Type::I64, Operand::Value(frame), FRAME_STATE_OFF),
        Type::I64,
        None,
    );
    let mut targets: Vec<(i64, BlockId)> = Vec::new();
    for (k, offs) in words.iter().enumerate() {
        if offs.is_empty() {
            continue;
        }
        let b = fresh_block(trace);
        for &off in offs {
            let base = trace.append_inst(
                b,
                InstKind::PtrToInt(Operand::Value(frame)),
                Type::I64,
                None,
            );
            let sum = InstKind::BinOp(
                BinOp::Add,
                Operand::Value(base),
                Operand::ConstI64(off as i64),
            );
            let sum = trace.append_inst(b, sum, Type::I64, None);
            let addr =
                trace.append_inst(b, InstKind::IntToPtr(Operand::Value(sum)), Type::Ptr, None);
            let load = InstKind::Load(Type::Ptr, Operand::Value(frame), off);
            let child = trace.append_inst(b, load, Type::Ptr, None);
            let idx = Operand::ConstI64(TRACE_STATE_WORD | (off / 8) as i64);
            let args = vec![
                idx,
                Operand::Value(child),
                Operand::Value(addr),
                Operand::Value(ctx),
            ];
            trace.append_void(
                b,
                InstKind::CallIndirect(visit_sig, Operand::Value(visit), args),
            );
        }
        trace.blocks[b.0 as usize].term = Terminator::Br(exit);
        targets.push((k as i64 + 1, b));
    }
    let state = Operand::Value(state);
    super::branch::on_state(trace, entry, state, &targets, Some(exit));
}
